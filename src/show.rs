use std::{
    sync::{Arc, mpsc::Sender},
    time::{Duration, Instant},
};

use crate::{
    animation::AnimationUIState,
    channel::{ChannelStateEmitter, Channels, strobe_control_channel},
    clocks::Clocks,
    color::Hsluv,
    control::{ControlMessage, Controller, MetaCommand, meta_command_from_osc},
    dmx::DmxUniverse,
    fixture::{
        Patch, animation_target::ControllableTargetedAnimation, prelude::FixtureGroupUpdate,
    },
    gui_state::{
        AnimationSnapshot, DMX_DEBUG_NOT_WATCHING, DmxDebugSnapshot, DmxPortInfo, DmxPortStatus,
        PatchSnapshot, SharedGuiState, StateDirty,
    },
    master::MasterControls,
    midi::{EmitMidiChannelMessage, MidiControlMessage, MidiHandler, slots},
    osc::{OscControlMessage, ScopedControlEmitter},
    preview::Previewer,
};

use tunnels::audio::EnvelopeStreams;

use anyhow::{Context, Result, bail};
use color_organ::{HsluvColor, IgnoreEmitter};
use log::{debug, error, warn};
use rust_dmx::{DmxPort, OfflineDmxPort};

pub struct Show {
    controller: Controller,
    dmx: Vec<DmxUniverse>,
    patch: Patch,
    channels: Channels,
    master_controls: MasterControls,
    animation_ui_state: AnimationUIState,
    clocks: Clocks,
    preview: Previewer,
    master_strobe_channel: Option<usize>,
    gui_state: SharedGuiState,
    envelope_streams_tx: Sender<EnvelopeStreams>,
    /// Last time a DMX output debug snapshot was pushed, for rate limiting.
    last_dmx_debug: Instant,
    /// Path to the show file on disk, if one is bound. `None` disables
    /// persistence.
    show_file_path: Option<crate::show_file::ShowPath>,
    /// Worker that performs the actual file write off the show thread.
    saver: crate::show_saver::ShowSaver,
}

const CONTROL_TIMEOUT: Duration = Duration::from_micros(500);
/// Minimum interval between DMX output debug snapshots (~4fps). The debug
/// window only needs a coarse view of output, so we throttle well below the
/// show framerate to keep the snapshot off the hot path.
const DMX_DEBUG_INTERVAL: Duration = Duration::from_millis(250);
/// The enttec hypothetically outputs 40 fps. This seems to only truly be the
/// case when no writes are being performed. Writing at the port framerate (or
/// even twice as fast) seems to bring the framerate down a bit - adding about
/// 300 us or so in between frames. Thus this slightly odd update interval.
pub const UPDATE_INTERVAL: Duration = Duration::from_micros(25300);

impl Show {
    #[expect(clippy::too_many_arguments)]
    pub fn new(
        patch: Patch,
        show_file_path: Option<crate::show_file::ShowPath>,
        controller: Controller,
        dmx: Vec<DmxUniverse>,
        clocks: Clocks,
        preview: Previewer,
        gui_state: SharedGuiState,
        envelope_streams_tx: Sender<EnvelopeStreams>,
    ) -> Result<Self> {
        let channels = Channels::new(&patch);
        let initial_channel = channels.current_channel();
        let animation_ui_state = AnimationUIState::new(initial_channel);

        let initial_groups = patch.configs();
        let mut show = Self {
            controller,
            dmx,
            patch,
            channels,
            master_controls: Default::default(),
            animation_ui_state,
            clocks,
            preview,
            master_strobe_channel: None,
            gui_state,
            envelope_streams_tx,
            last_dmx_debug: Instant::now(),
            show_file_path,
            saver: crate::show_saver::ShowSaver::spawn(),
        };
        show.reconcile_submaster_wings()?;
        show.reconcile_clock_wing()?;
        show.refresh_ui();
        show.snapshot_state(StateDirty::GUI_ALL);
        // Initial save: surfaces a bad path early and normalizes the
        // on-disk file to the current schema if the load reconciled
        // anything.
        show.save_show();
        show.gui_state.patch_snapshot.store(Arc::new(PatchSnapshot {
            groups: initial_groups,
        }));
        Ok(show)
    }

    /// Submit a snapshot of the current show state for persistence. No-op
    /// when no show file path is bound.
    fn save_show(&self) {
        let Some(path) = self.show_file_path.as_ref() else {
            return;
        };
        let file = crate::show_file::ShowFile {
            patch: self.patch.configs(),
            positioners: self
                .patch
                .iter()
                .filter_map(|g| g.positioner().map(|p| (g.id(), p.presets().clone())))
                .collect(),
        };
        self.saver.submit(path.clone(), file);
    }

    /// Run the show forever in the current thread.
    pub fn run(&mut self) {
        let mut last_update = Instant::now();

        loop {
            // Process a control event if one is pending.
            match self.control(CONTROL_TIMEOUT) {
                Ok(dirty) => {
                    if !dirty.is_empty() {
                        self.snapshot_state(dirty);
                    }
                }
                Err(err) => error!("A control error occurred: {err:#}."),
            }

            // Compute updates until we're current.
            let mut now = Instant::now();
            let mut time_since_last_update = now - last_update;
            let mut should_render = false;
            while time_since_last_update > UPDATE_INTERVAL {
                // Update the state of the show.
                self.update(UPDATE_INTERVAL);
                should_render = true;

                last_update += UPDATE_INTERVAL;
                now = Instant::now();
                time_since_last_update = now - last_update;
            }

            // Render the state of the show.
            if should_render {
                self.render();
                for univ in &mut self.dmx {
                    if let Err(e) = univ.port.write(&univ.buffer) {
                        warn!("DMX write error: {e:#}.");
                    }
                }
                self.snapshot_dmx_debug();
            }
        }
    }

    /// Handle at most one control message.
    ///
    /// Wait for the provided duration for a message to appear.
    fn control(&mut self, timeout: Duration) -> Result<StateDirty> {
        let msg = match self.controller.recv(timeout)? {
            Some(m) => m,
            None => {
                return Ok(StateDirty::CLEAN);
            }
        };

        match msg {
            ControlMessage::MidiDeviceChange(change) => {
                let needs_ui_refresh = self.controller.handle_device_change(change)?;
                if needs_ui_refresh {
                    self.refresh_ui();
                }
                Ok(StateDirty::MIDI_SLOTS)
            }
            ControlMessage::Midi(msg) => self.handle_midi_message(&msg),
            ControlMessage::Osc(msg) => self.handle_osc_message(&msg),
            ControlMessage::Meta(cmd, reply) => {
                let result = self.handle_meta_command(cmd);
                if let Some(reply) = reply {
                    let _ = reply.send(result.as_ref().map(|_| ()).map_err(|e| format!("{e:#}")));
                }
                result
            }
        }
    }

    /// Handle a meta-command.
    fn handle_meta_command(&mut self, cmd: MetaCommand) -> Result<StateDirty> {
        match cmd {
            MetaCommand::Repatch(groups) => {
                self.patch.repatch(Arc::clone(&groups))?;
                self.gui_state
                    .patch_snapshot
                    .store(Arc::new(PatchSnapshot { groups }));
                self.post_repatch().map(|d| d | StateDirty::SHOW_FILE)
            }
            MetaCommand::RefreshUI => {
                self.refresh_ui();
                Ok(StateDirty::GUI_ALL)
            }
            MetaCommand::ResetAllAnimations => {
                for group in self.patch.iter_mut() {
                    group.reset_animations();
                }
                self.refresh_ui();
                Ok(StateDirty::CLEAN)
            }
            MetaCommand::AssignDmxPort { universe, port } => {
                assign_dmx_port(&mut self.dmx, universe, port)?;
                Ok(StateDirty::DMX_PORTS)
            }
            MetaCommand::SetDmxPortFramerate {
                universe,
                framerate,
            } => {
                let univ = self
                    .dmx
                    .get_mut(universe)
                    .with_context(|| format!("universe {universe} out of range"))?;
                univ.port
                    .set_framerate(framerate)
                    .with_context(|| format!("set framerate on port {}", univ.port))?;
                Ok(StateDirty::DMX_PORTS)
            }
            MetaCommand::ClearMidiDevice { slot_name } => {
                self.controller.clear_midi_device(&slot_name)?;
                self.refresh_ui();
                Ok(StateDirty::MIDI_SLOTS)
            }
            MetaCommand::ConnectMidiPort {
                slot_name,
                device_id,
                kind,
            } => {
                self.controller
                    .connect_midi_port(&slot_name, device_id, kind)?;
                self.refresh_ui();
                Ok(StateDirty::MIDI_SLOTS)
            }
            MetaCommand::UseClockService(service) => {
                self.clocks = Clocks::Service(service);
                self.reconcile_clock_wing()?;
                self.refresh_ui();
                Ok(StateDirty::MIDI_SLOTS | StateDirty::CLOCK_STATE)
            }
            MetaCommand::UseInternalClocks(device_name) => {
                self.clocks = Clocks::internal(device_name, self.envelope_streams_tx.clone())?;
                self.reconcile_clock_wing()?;
                self.refresh_ui();
                Ok(StateDirty::MIDI_SLOTS | StateDirty::CLOCK_STATE | StateDirty::AUDIO)
            }
            MetaCommand::SetClockWingModel(model) => {
                self.controller
                    .reconcile_clock_wing(self.clocks.is_internal(), model)?;
                Ok(StateDirty::MIDI_SLOTS)
            }
            MetaCommand::RegisterOscClient(client_id) => {
                println!("Registering new OSC client at {client_id}.");
                self.controller.register_osc_client(client_id);
                self.refresh_ui();
                Ok(StateDirty::OSC_CLIENTS)
            }
            MetaCommand::DropOscClient(client_id) => {
                println!("Deregistering OSC client at {client_id}.");
                self.controller.deregister_osc_client(client_id);
                Ok(StateDirty::OSC_CLIENTS)
            }
            MetaCommand::SwapOscSocket(socket) => {
                self.controller.swap_osc_socket(socket);
                Ok(StateDirty::CLEAN)
            }
            MetaCommand::SetMasterStrobeChannel(enable) => {
                if enable {
                    let ch = self.resolve_strobe_channel().context(
                        "cannot enable master strobe: last wing fader is occupied by a fixture group",
                    )?;
                    self.set_master_strobe_channel(Some(ch));
                } else {
                    self.set_master_strobe_channel(None);
                }
                Ok(StateDirty::CLEAN)
            }
            MetaCommand::AudioControl(msg) => {
                Ok(self.clocks.control_audio(msg, &mut self.controller))
            }
            MetaCommand::RenamePositionerPreset(name) => {
                let Some(channel) = self.channels.current_channel() else {
                    return Ok(StateDirty::CLEAN);
                };
                let group = self.patch.channel_group_mut(channel)?;
                let (group_name, positioner) = group.split_for_positioner_dispatch();
                let Some(positioner) = positioner else {
                    return Ok(StateDirty::CLEAN);
                };
                let sender = self.controller.sender_with_metadata(None);
                let emitter = crate::osc::FixtureStateEmitter::new(
                    group_name,
                    ChannelStateEmitter::new(
                        crate::channel::ChannelBinding::Current(channel),
                        &sender,
                    ),
                );
                positioner.rename_active_preset(name, &emitter);
                Ok(StateDirty::SHOW_FILE)
            }
        }
    }

    /// Shared post-repatch logic: clear channels, reconcile wings, resize DMX buffers.
    fn post_repatch(&mut self) -> Result<StateDirty> {
        let sender = self.controller.sender_with_metadata(None);
        sender.emit_midi_channel_message(&crate::channel::StateChange::Clear);
        Channels::emit_osc_state_change(
            crate::channel::StateChange::Clear,
            &ScopedControlEmitter {
                entity: crate::osc::channels::GROUP,
                emitter: &sender,
            },
        );
        self.channels.reconcile_to_patch(&self.patch);
        if self.master_strobe_channel.is_some() {
            // Re-resolve: channel may have moved to a new wing or become occupied.
            self.set_master_strobe_channel(self.resolve_strobe_channel());
        }
        self.reconcile_submaster_wings()?;
        self.refresh_ui();
        let new_universe_count = self.patch.universe_count();
        let current_len = self.dmx.len();
        if new_universe_count > current_len {
            for _ in current_len..new_universe_count {
                self.dmx.push(DmxUniverse::offline());
            }
        } else if new_universe_count < current_len {
            self.dmx.truncate(new_universe_count);
        }
        for univ in &mut self.dmx {
            univ.buffer.fill(0);
        }
        Ok(StateDirty::MIDI_SLOTS | StateDirty::DMX_PORTS)
    }

    /// Returns `Some(channel_index)` if the last wing fader is available for
    /// strobe, `None` if it is occupied by a fixture group.
    fn resolve_strobe_channel(&self) -> Option<usize> {
        let ch = strobe_control_channel(self.patch.channel_count());
        if ch < self.patch.channel_count() {
            None // channel is occupied by a fixture group
        } else {
            Some(ch) // available
        }
    }

    /// Update the strobe channel state and sync to GUI.
    fn set_master_strobe_channel(&mut self, value: Option<usize>) {
        self.master_strobe_channel = value;
        self.gui_state
            .master_strobe_fader_channel_mapped
            .store(value.is_some(), std::sync::atomic::Ordering::Relaxed);
    }

    /// Handle a channel control message, routing to strobe or fixture groups.
    fn handle_channel_message(&mut self, msg: &crate::channel::ControlMessage) -> Result<()> {
        let sender = self.controller.sender_with_metadata(None);
        if let Some(strobe_ch) = self.master_strobe_channel
            && let crate::channel::ControlMessage::Control { channel_id, msg } = msg
            && *channel_id == Some(strobe_ch)
        {
            self.master_controls.handle_strobe_channel(msg, &sender);
        } else {
            self.channels
                .control(msg, &mut self.patch, &self.animation_ui_state, &sender)?;
        }
        Ok(())
    }

    /// Handle a single MIDI control message.
    fn handle_midi_message(&mut self, msg: &MidiControlMessage) -> Result<StateDirty> {
        let sender = self.controller.sender_with_metadata(None);
        let Some(show_ctrl_msg) = msg.device.interpret(&msg.event) else {
            return Ok(StateDirty::CLEAN);
        };
        match show_ctrl_msg {
            ShowControlMessage::Channel(msg) => {
                self.handle_channel_message(&msg)?;
                Ok(StateDirty::CLEAN)
            }
            ShowControlMessage::Clock(msg) => {
                self.clocks.control_clock(msg, sender.controller);
                Ok(StateDirty::CLEAN)
            }
            ShowControlMessage::Audio(msg) => {
                Ok(self.clocks.control_audio(msg, &mut self.controller))
            }
            ShowControlMessage::Master(msg) => {
                self.master_controls.control(&msg, &sender);
                Ok(StateDirty::CLEAN)
            }
            ShowControlMessage::Animation(msg) => {
                let Some(channel) = self.channels.current_channel() else {
                    // An animation control message with no channel selected is an
                    // expected transient input condition, not a fault — ignore it.
                    debug!("ignoring animation control message with no channel selected\n{msg:?}");
                    return Ok(StateDirty::CLEAN);
                };
                let group = self.patch.channel_group_mut(channel)?;
                self.animation_ui_state.control(
                    msg,
                    channel,
                    group,
                    &ScopedControlEmitter {
                        entity: crate::osc::animation::GROUP,
                        emitter: &sender,
                    },
                )?;
                Ok(StateDirty::CLEAN)
            }
            ShowControlMessage::ColorOrgan(msg) => {
                // FIXME: this is really janky and has no way to route messages.
                for group in self.patch.iter_mut() {
                    let Some(color_organ) = group.color_organ_mut() else {
                        continue;
                    };
                    color_organ.control(msg.clone(), &IgnoreEmitter);
                }
                Ok(StateDirty::CLEAN)
            }
        }
    }

    /// Handle a single OSC message.
    fn handle_osc_message(&mut self, msg: &OscControlMessage) -> Result<StateDirty> {
        let sender = self.controller.sender_with_metadata(Some(&msg.client_id));

        match msg.group() {
            "Meta" => {
                if let Some(cmd) = meta_command_from_osc(msg)? {
                    self.handle_meta_command(cmd)
                } else {
                    Ok(StateDirty::CLEAN)
                }
            }
            crate::master::GROUP => {
                self.master_controls.control_osc(msg, &sender)?;
                Ok(StateDirty::CLEAN)
            }
            crate::osc::channels::GROUP => {
                self.channels.control_osc(
                    msg,
                    &mut self.patch,
                    &self.animation_ui_state,
                    &sender,
                )?;
                Ok(StateDirty::CLEAN)
            }
            crate::osc::animation::GROUP => {
                let Some(channel) = self.channels.current_channel() else {
                    // An animation control message with no channel selected is an
                    // expected transient input condition, not a fault — ignore it.
                    debug!("ignoring animation control message with no channel selected\n{msg:?}");
                    return Ok(StateDirty::CLEAN);
                };
                let group = self.patch.channel_group_mut(channel)?;
                self.animation_ui_state.control_osc(
                    msg,
                    channel,
                    group,
                    &ScopedControlEmitter {
                        entity: crate::osc::animation::GROUP,
                        emitter: &sender,
                    },
                )?;
                Ok(StateDirty::CLEAN)
            }
            crate::osc::audio::GROUP => self.clocks.control_audio_osc(msg, &mut self.controller),
            crate::osc::clock::GROUP => {
                self.clocks.control_clock_osc(msg, &mut self.controller)?;
                Ok(StateDirty::CLEAN)
            }
            crate::osc::positioner::GROUP => {
                // /Positioner/... dispatch. Look up the current channel's
                // group; if it has a positioner, hand the message off with
                // a `ChannelBinding::Current` emitter (we know we're in
                // current-channel context by construction).
                let Some(channel) = self.channels.current_channel() else {
                    return Ok(StateDirty::CLEAN);
                };
                let group = self.patch.channel_group_mut(channel)?;
                let channel_emitter = ChannelStateEmitter::new(
                    crate::channel::ChannelBinding::Current(channel),
                    &sender,
                );
                // The fixture emitter and the positioner_mut borrow target
                // different fields of `group`; spell that out via let
                // bindings so the borrow checker can split them.
                let (name, positioner) = group.split_for_positioner_dispatch();
                if let Some(positioner) = positioner {
                    let fixture_emitter =
                        crate::osc::FixtureStateEmitter::new(name, channel_emitter);
                    if positioner.control_osc_positioner_scoped(msg, &fixture_emitter)? {
                        return Ok(StateDirty::SHOW_FILE);
                    }
                }
                Ok(StateDirty::CLEAN)
            }
            // Assume any other control group is referring to a fixture group.
            fixture_group => {
                let (group, channel_id) = self.patch.lookup_mut_by_name(fixture_group)?;
                let binding = crate::channel::ChannelBinding::resolve(
                    channel_id,
                    self.channels.current_channel(),
                );
                group.control(msg, ChannelStateEmitter::new(binding, &sender))?;
                Ok(StateDirty::CLEAN)
            }
        }
    }

    /// Drive save and GUI snapshot reactions to dirty state. For each set
    /// flag, performs the corresponding downstream reconciliation: saving
    /// the show file to disk or refreshing the matching GUI snapshot.
    fn snapshot_state(&self, dirty: StateDirty) {
        if dirty.contains(StateDirty::SHOW_FILE) {
            self.save_show();
        }
        if dirty.contains(StateDirty::MIDI_SLOTS) {
            self.gui_state
                .midi_slots
                .store(self.controller.midi_slot_statuses());
        }
        if dirty.contains(StateDirty::OSC_CLIENTS) {
            self.gui_state
                .osc_clients
                .store(self.controller.osc_client_ids());
        }
        if dirty.contains(StateDirty::CLOCK_STATE) {
            self.gui_state
                .clock_status
                .store(Arc::new(self.clocks.status()));
        }
        if dirty.contains(StateDirty::DMX_PORTS) {
            self.gui_state
                .dmx_port_status
                .store(Arc::new(DmxPortStatus {
                    ports: self
                        .dmx
                        .iter()
                        .map(|u| DmxPortInfo {
                            name: u.port.to_string(),
                            framerate: u.port.get_framerate(),
                        })
                        .collect(),
                }));
        }
        if dirty.contains(StateDirty::AUDIO)
            && let Some(snap) = self.clocks.audio_snapshot()
        {
            self.gui_state.audio_state.store(snap);
        }
    }

    /// Update the state of the show using the provided timestep.
    fn update(&mut self, delta_t: Duration) {
        self.clocks.update(delta_t, &mut self.controller);

        let clock_state = self.clocks.get();
        self.master_controls.clock_state = clock_state.clock_bank;
        self.master_controls.audio_envelope = clock_state.audio_envelope;

        self.master_controls
            .update(delta_t, &self.controller.sender_with_metadata(None));

        let mut flash_distributor = self
            .master_controls
            .flash_distributor(self.patch.iter().filter(|g| g.strobe_enabled()).count());

        for group in self.patch.iter_mut() {
            group.update(
                FixtureGroupUpdate {
                    master_controls: &self.master_controls,
                    flash_now: flash_distributor.flash_now(group.strobe_enabled()),
                },
                delta_t,
            );
        }

        if let Err(err) = self.snapshot_animation_state() {
            warn!("Animation state snapshot error: {err}.");
        };
    }

    /// Render the state of the show out to DMX.
    fn render(&mut self) {
        self.preview.start_frame();
        // NOTE: we don't bother to empty the buffer because we will always
        // overwrite all previously-rendered state.
        for group in self.patch.iter() {
            group.render(&self.master_controls, &mut self.dmx, &self.preview);
        }
    }

    /// Push the current output buffer for the watched universe to the GUI, if
    /// the DMX output debug window is open. Throttled to `DMX_DEBUG_INTERVAL`
    /// (~4fps) and a no-op when no window is watching.
    fn snapshot_dmx_debug(&mut self) {
        let watched = self
            .gui_state
            .dmx_debug_watch
            .load(std::sync::atomic::Ordering::Relaxed);
        if watched == DMX_DEBUG_NOT_WATCHING {
            return;
        }
        if self.last_dmx_debug.elapsed() < DMX_DEBUG_INTERVAL {
            return;
        }
        // Out of range after a repatch shrank the universe count — skip until
        // the GUI selects a valid universe.
        let Some(univ) = self.dmx.get(watched) else {
            return;
        };
        self.gui_state.dmx_debug.store(Some(DmxDebugSnapshot {
            universe: watched,
            values: univ.buffer,
        }));
        self.last_dmx_debug = Instant::now();
    }

    /// Reconcile MIDI submaster wing slots with the current channel count.
    fn reconcile_submaster_wings(&mut self) -> Result<()> {
        self.controller
            .reconcile_submaster_wings(self.patch.channel_count())
    }

    /// Reconcile the clock wing slot with the current clock mode, preserving
    /// the model of an existing slot.
    fn reconcile_clock_wing(&mut self) -> Result<()> {
        let model = self
            .controller
            .midi_slot_statuses()
            .into_iter()
            .find(|s| s.name == slots::CLOCK_WING_SLOT)
            .and_then(|s| slots::clock_wing_by_name(&s.model))
            .unwrap_or(slots::DEFAULT_CLOCK_WING);
        self.controller
            .reconcile_clock_wing(self.clocks.is_internal(), model)
    }

    /// Send messages to refresh all UI state.
    fn refresh_ui(&mut self) {
        let emitter = &self.controller.sender_with_metadata(None);
        let current_channel = self.channels.current_channel();
        for group in self.patch.iter() {
            let binding = crate::channel::ChannelBinding::resolve(
                self.patch.channel_for_id(group.id()),
                current_channel,
            );
            group.emit_state(ChannelStateEmitter::new(binding, emitter));
        }

        self.master_controls.emit_state(emitter);

        self.channels.emit_state(false, &self.patch, emitter);

        let positioner_emitter = ScopedControlEmitter {
            entity: crate::osc::positioner::GROUP,
            emitter,
        };
        if let Some(current_channel) = self.channels.current_channel() {
            match self.patch.channel_group(current_channel) {
                Ok(group) => {
                    self.animation_ui_state.emit_state(
                        current_channel,
                        group,
                        &ScopedControlEmitter {
                            entity: crate::osc::animation::GROUP,
                            emitter,
                        },
                    );
                    if let Some(positioner) = group.positioner() {
                        positioner.emit_positioner_state(&positioner_emitter);
                    } else {
                        crate::positioner::emit_cleared_positioner_state(&positioner_emitter);
                    }
                }
                Err(e) => error!("{e:#}"),
            }
        } else {
            // No current channel at all (e.g. empty patch at cold start).
            // Same cleared state as the non-positionable case.
            crate::positioner::emit_cleared_positioner_state(&positioner_emitter);
        }

        self.clocks.emit_state(&mut self.controller);
    }

    /// Snapshot animation state into the shared GUI state, if the visualizer is active.
    fn snapshot_animation_state(&mut self) -> Result<()> {
        if !self
            .gui_state
            .visualizer_active
            .load(std::sync::atomic::Ordering::Relaxed)
        {
            return Ok(());
        }
        let Some(current_channel) = self.channels.current_channel() else {
            return Ok(());
        };
        let group = self.patch.channel_group(current_channel)?;
        let animation_index = self
            .animation_ui_state
            .animation_index_for_channel(current_channel);
        self.gui_state
            .animation_state
            .store(Arc::new(AnimationSnapshot {
                animation: group
                    .get_animation(animation_index)
                    .map(ControllableTargetedAnimation::anim)
                    .cloned()
                    .unwrap_or_default(),
                clocks: self.clocks.get(),
                fixture_count: group.fixture_configs().len(),
            }));
        Ok(())
    }
}

/// Assign a DMX port to a universe.
///
/// Validates the universe index, opens the port, zeros the DMX buffer,
/// and swaps the port into place.
fn assign_dmx_port(
    dmx: &mut [DmxUniverse],
    universe: usize,
    mut port: Box<dyn DmxPort>,
) -> Result<()> {
    if dmx.get(universe).is_none() {
        bail!(
            "universe {universe} out of range (show has {} universe(s))",
            dmx.len()
        );
    }
    // Prevent assigning the same port to multiple universes.
    let new_name = port.to_string();
    let offline_name = OfflineDmxPort.to_string();
    if new_name != offline_name {
        for (i, existing) in dmx.iter().enumerate() {
            if i != universe && existing.port.to_string() == new_name {
                bail!("port {new_name} is already assigned to universe {i}");
            }
        }
    }
    port.open()
        .map_err(|e| anyhow::anyhow!("failed to open port {port}: {e}"))?;
    dmx[universe].buffer.fill(0);
    dmx[universe].port = port;
    Ok(())
}

/// Strongly-typed top-level show control messages.
/// These cover all of the fixed control features, but not fixture-specific controls.
#[derive(Debug, Clone)]
pub enum ShowControlMessage {
    // Unused because show control messages only come from OSC so far.
    #[allow(unused)]
    Master(crate::master::ControlMessage),
    Channel(crate::channel::ControlMessage),
    Clock(tunnels::clock_bank::ControlMessage),
    // Unused because show control messages only come from OSC so far.
    #[allow(unused)]
    Animation(crate::animation::ControlMessage),
    Audio(tunnels::audio::ControlMessage),
    ColorOrgan(color_organ::ControlMessage<Hsluv>),
}

impl From<Hsluv> for HsluvColor {
    fn from(c: Hsluv) -> Self {
        Self::new(c.hue, c.sat, c.lightness)
    }
}

#[cfg(test)]
impl Show {
    fn test_new(patch: Patch) -> (Self, std::sync::mpsc::Sender<ControlMessage>) {
        let (show, send, _osc_recv) = Self::test_new_inner(patch, |_tx| Clocks::test_new());
        (show, send)
    }

    fn test_new_internal(patch: Patch) -> (Self, std::sync::mpsc::Sender<ControlMessage>) {
        let (show, send, _osc_recv) = Self::test_new_inner(patch, |tx| {
            Clocks::internal(None, tx).expect("internal clocks should construct in test")
        });
        (show, send)
    }

    /// Variant of `test_new` that surfaces the OSC response receiver so the
    /// caller can capture what the Show emits in response to incoming OSC
    /// messages. See `tests::OscCapture` for the wrapper integration tests
    /// use around the raw receiver.
    pub(crate) fn test_new_with_osc_capture(
        patch: Patch,
    ) -> (
        Self,
        std::sync::mpsc::Sender<ControlMessage>,
        std::sync::mpsc::Receiver<crate::osc::OscControlResponse>,
    ) {
        Self::test_new_inner(patch, |_tx| Clocks::test_new())
    }

    fn test_new_inner(
        patch: Patch,
        build_clocks: impl FnOnce(std::sync::mpsc::Sender<EnvelopeStreams>) -> Clocks,
    ) -> (
        Self,
        std::sync::mpsc::Sender<ControlMessage>,
        std::sync::mpsc::Receiver<crate::osc::OscControlResponse>,
    ) {
        let (controller, send, osc_recv) = Controller::test_new();
        let universe_count = patch.universe_count();
        let channels = Channels::new(&patch);
        let initial_channel = channels.current_channel();
        let (envelope_streams_tx, _envelope_rx) = std::sync::mpsc::channel();
        let clocks = build_clocks(envelope_streams_tx.clone());
        let initial_clock_status = clocks.status();
        let gui_state: SharedGuiState = Arc::new(crate::gui_state::GuiState::new(
            vec![],
            initial_clock_status,
            None,
            tunnels_lib::repaint::noop_repaint(),
            tunnels_lib::repaint::noop_repaint(),
        ));
        let mut show = Self {
            controller,
            dmx: (0..universe_count)
                .map(|_| DmxUniverse::offline())
                .collect(),
            patch,
            channels,
            master_controls: Default::default(),
            animation_ui_state: AnimationUIState::new(initial_channel),
            clocks,
            preview: Previewer::Off,
            master_strobe_channel: None,
            gui_state,
            envelope_streams_tx,
            last_dmx_debug: Instant::now(),
            show_file_path: None,
            saver: crate::show_saver::ShowSaver::spawn(),
        };
        show.reconcile_submaster_wings().unwrap();
        show.reconcile_clock_wing().unwrap();
        (show, send, osc_recv)
    }

    /// Read-only access to the patch, for test assertions about in-memory
    /// state. Production code should access groups through proper OSC /
    /// MetaCommand entry points, not by reaching around the Show.
    #[cfg(test)]
    pub(crate) fn patch_for_test(&self) -> &Patch {
        &self.patch
    }

    /// Read-only access to the channel-selection state, for test assertions.
    #[cfg(test)]
    pub(crate) fn channels_for_test(&self) -> &Channels {
        &self.channels
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dmx::mock::MockDmxPort;

    const ONE_UNIVERSE_PATCH: &str = "\
- fixture: Dimmer
  patches:
    - addr: 1
";
    const TWO_UNIVERSE_PATCH: &str = "\
- fixture: Dimmer
  patches:
    - addr: 1
    - addr: 1
      universe: 1
";

    fn show_from_yaml(yaml: &str) -> Show {
        let configs: Vec<crate::config::FixtureGroupConfig> = serde_yaml::from_str(yaml).unwrap();
        let patch = Patch::patch_all(configs.into()).unwrap();
        let (show, _send) = Show::test_new(patch);
        show
    }

    fn show_internal_from_yaml(yaml: &str) -> Show {
        let configs: Vec<crate::config::FixtureGroupConfig> = serde_yaml::from_str(yaml).unwrap();
        let patch = Patch::patch_all(configs.into()).unwrap();
        let (show, _send) = Show::test_new_internal(patch);
        show
    }

    // === OSC integration test harness ===========================================
    //
    // The harness has three layers, each independently useful:
    //
    // 1. `show_with_capture_from_yaml` — builds a Show wired to in-process OSC
    //    channels and returns it alongside an `OscCapture` that drains whatever
    //    the Show emits in response to incoming OSC messages.
    //
    // 2. `OscCapture` — drains the OSC response channel and exposes ergonomic
    //    drain/assert helpers.
    //
    // 3. `fire` / `fire_press` — build an `OscControlMessage` from a string
    //    address + arg, dispatch it through `Show::handle_osc_message`, and
    //    return the resulting `StateDirty`.
    //
    // Typical flow:
    //
    // ```rust
    // let (mut show, capture, _send) = show_with_capture_from_yaml(SOME_PATCH);
    // fire(&mut show, "/Channels/Select/1/1", OscType::Float(1.0)).unwrap();
    // capture.drain();  // discard the initial selection emit
    // fire_press(&mut show, "/Positioner/Preset/1/3").unwrap();
    // let emits = capture.drain_by_addr();
    // assert_eq!(emits.get("/Positioner/Preset/1/3"), Some(&OscType::Float(1.0)));
    // ```
    use crate::osc::{OscClientId, OscControlResponse};
    use rosc::{OscMessage, OscType};
    use std::collections::HashMap;
    use std::sync::mpsc::Receiver;

    /// Wraps the OSC response receiver with drain/assert helpers tailored to
    /// the patterns integration tests use.
    pub(crate) struct OscCapture {
        rx: Receiver<OscControlResponse>,
    }

    impl OscCapture {
        fn new(rx: Receiver<OscControlResponse>) -> Self {
            Self { rx }
        }

        /// Drain every response pending on the channel right now. Non-blocking;
        /// returns whatever has accumulated since the last drain.
        pub fn drain(&self) -> Vec<OscControlResponse> {
            let mut out = Vec::new();
            while let Ok(r) = self.rx.try_recv() {
                out.push(r);
            }
            out
        }

        /// Drain and return as a HashMap keyed by full OSC address. If multiple
        /// emits target the same address, the last one wins — which matches
        /// TouchOSC display semantics (we care about the final visible state).
        pub fn drain_by_addr(&self) -> HashMap<String, OscType> {
            self.drain()
                .into_iter()
                .map(|r| {
                    (
                        r.msg.addr,
                        r.msg.args.into_iter().next().unwrap_or(OscType::Nil),
                    )
                })
                .collect()
        }
    }

    /// Build a Show wired to in-process channels, with a paired `OscCapture`
    /// that captures everything emitted from `Show::handle_osc_message`.
    fn show_with_capture_from_yaml(
        yaml: &str,
    ) -> (Show, OscCapture, std::sync::mpsc::Sender<ControlMessage>) {
        let configs: Vec<crate::config::FixtureGroupConfig> = serde_yaml::from_str(yaml).unwrap();
        let patch = Patch::patch_all(configs.into()).unwrap();
        let (show, send, rx) = Show::test_new_with_osc_capture(patch);
        (show, OscCapture::new(rx), send)
    }

    /// Build and dispatch an OSC message at `addr` carrying a single `arg`,
    /// returning the resulting `StateDirty`.
    fn fire(show: &mut Show, addr: &str, arg: OscType) -> Result<StateDirty> {
        let msg = crate::osc::OscControlMessage::new(
            OscMessage {
                addr: addr.to_string(),
                args: vec![arg],
            },
            OscClientId::example(),
        )
        .expect("test address parses");
        show.handle_osc_message(&msg)
    }

    /// Convenience for momentary button presses — fires `Float(1.0)`.
    fn fire_press(show: &mut Show, addr: &str) -> Result<StateDirty> {
        fire(show, addr, OscType::Float(1.0))
    }

    #[test]
    fn meta_command_audio_control_marks_audio_dirty() {
        let mut show = show_internal_from_yaml(ONE_UNIVERSE_PATCH);

        let dirty = show
            .handle_meta_command(MetaCommand::AudioControl(
                tunnels::audio::ControlMessage::ResetParameters,
            ))
            .expect("audio control should not error");

        assert_eq!(dirty, StateDirty::AUDIO);
    }

    #[test]
    fn meta_command_use_internal_clocks_marks_full_dirty_mask() {
        // Start in service mode, switch to internal — exercises the rebuild path.
        let mut show = show_from_yaml(ONE_UNIVERSE_PATCH);

        let dirty = show
            .handle_meta_command(MetaCommand::UseInternalClocks(None))
            .expect("use internal clocks should not error");

        assert_eq!(
            dirty,
            StateDirty::MIDI_SLOTS | StateDirty::CLOCK_STATE | StateDirty::AUDIO,
        );
    }

    fn clock_wing_model(show: &Show) -> Option<String> {
        show.controller
            .midi_slot_statuses()
            .into_iter()
            .find(|s| s.name == slots::CLOCK_WING_SLOT)
            .map(|s| s.model)
    }

    #[test]
    fn meta_command_set_clock_wing_model_rebuilds_slot() {
        use crate::midi::{Device, device::amx::AkaiAmx};
        use tunnels::midi_controls::MidiDevice;

        // Internal clocks → clock wing slot is present with the default model.
        let mut show = show_internal_from_yaml(ONE_UNIVERSE_PATCH);
        assert_eq!(
            clock_wing_model(&show).as_deref(),
            Some(slots::DEFAULT_CLOCK_WING.device_name())
        );

        let amx = Device::Amx(AkaiAmx {});
        let dirty = show
            .handle_meta_command(MetaCommand::SetClockWingModel(amx))
            .expect("set clock wing model should not error");
        assert_eq!(dirty, StateDirty::MIDI_SLOTS);
        assert_eq!(clock_wing_model(&show).as_deref(), Some(amx.device_name()));

        // Switching back to the default rebuilds the slot again.
        show.handle_meta_command(MetaCommand::SetClockWingModel(slots::DEFAULT_CLOCK_WING))
            .expect("set clock wing model should not error");
        assert_eq!(
            clock_wing_model(&show).as_deref(),
            Some(slots::DEFAULT_CLOCK_WING.device_name())
        );
    }

    #[test]
    fn osc_client_register_drop_marks_dirty_and_snapshots() {
        let mut show = show_from_yaml(ONE_UNIVERSE_PATCH);
        let client = crate::osc::OscClientId::example();

        let dirty = show
            .handle_meta_command(MetaCommand::RegisterOscClient(client))
            .expect("register should not error");
        assert_eq!(dirty, StateDirty::OSC_CLIENTS);
        show.snapshot_state(dirty);
        assert!(show.gui_state.osc_clients.load().contains(&client));

        let dirty = show
            .handle_meta_command(MetaCommand::DropOscClient(client))
            .expect("drop should not error");
        assert_eq!(dirty, StateDirty::OSC_CLIENTS);
        show.snapshot_state(dirty);
        assert!(!show.gui_state.osc_clients.load().contains(&client));
    }

    #[test]
    fn assign_dmx_port_success() {
        let mut show = show_from_yaml(TWO_UNIVERSE_PATCH);
        show.dmx[1].buffer.fill(0xFF);

        show.handle_meta_command(MetaCommand::AssignDmxPort {
            universe: 1,
            port: Box::new(MockDmxPort::new()),
        })
        .unwrap();

        assert!(show.dmx[1].buffer.iter().all(|&b| b == 0));
        assert_eq!(format!("{}", show.dmx[1].port), "mock");
    }

    #[test]
    fn assign_dmx_port_open_fails() {
        let mut show = show_from_yaml(ONE_UNIVERSE_PATCH);

        let result = show.handle_meta_command(MetaCommand::AssignDmxPort {
            universe: 0,
            port: Box::new(MockDmxPort::failing()),
        });
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("failed to open port")
        );
    }

    #[test]
    fn assign_dmx_port_universe_out_of_range() {
        let mut show = show_from_yaml(TWO_UNIVERSE_PATCH);

        let result = show.handle_meta_command(MetaCommand::AssignDmxPort {
            universe: 5,
            port: Box::new(MockDmxPort::new()),
        });
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("out of range"));
        assert!(err_msg.contains("2 universe(s)"));
    }

    #[test]
    fn set_dmx_port_framerate() {
        let mut show = show_from_yaml(TWO_UNIVERSE_PATCH);

        // Universe 0 gets a framerate-capable port; universe 1 stays offline.
        show.handle_meta_command(MetaCommand::AssignDmxPort {
            universe: 0,
            port: Box::new(MockDmxPort::with_framerate(40)),
        })
        .unwrap();

        // Success: the supporting port accepts the new framerate and the
        // change is visible in the next snapshot.
        let dirty = show
            .handle_meta_command(MetaCommand::SetDmxPortFramerate {
                universe: 0,
                framerate: 30,
            })
            .unwrap();
        assert_eq!(dirty, StateDirty::DMX_PORTS);
        assert_eq!(show.dmx[0].port.get_framerate(), Some(30));

        show.snapshot_state(StateDirty::DMX_PORTS);
        let snapshot = show.gui_state.dmx_port_status.load();
        assert_eq!(snapshot.ports[0].framerate, Some(30));
        assert_eq!(snapshot.ports[1].framerate, None);

        // Unsupported: universe 1's offline port rejects set_framerate; the
        // error from `rust_dmx::SetFpsError::Unsupported` propagates with the
        // port name in the wrap.
        let err = show
            .handle_meta_command(MetaCommand::SetDmxPortFramerate {
                universe: 1,
                framerate: 30,
            })
            .unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains("set framerate on port offline"), "got: {msg}");
        assert!(
            msg.contains("does not support setting framerate"),
            "got: {msg}"
        );

        // Out of range: universe index past the end of the dmx vec.
        let err = show
            .handle_meta_command(MetaCommand::SetDmxPortFramerate {
                universe: 5,
                framerate: 30,
            })
            .unwrap_err();
        assert!(
            err.to_string().contains("universe 5 out of range"),
            "got: {err}"
        );
    }

    #[test]
    fn assign_dmx_port_rejects_duplicate() {
        let mut show = show_from_yaml(TWO_UNIVERSE_PATCH);

        // Assign a mock port to universe 0.
        show.handle_meta_command(MetaCommand::AssignDmxPort {
            universe: 0,
            port: Box::new(MockDmxPort::new()),
        })
        .unwrap();

        // Assigning the same port type to universe 1 should fail.
        let result = show.handle_meta_command(MetaCommand::AssignDmxPort {
            universe: 1,
            port: Box::new(MockDmxPort::new()),
        });
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("already assigned"));
    }

    fn repatch_from_yaml(show: &mut Show, yaml: &str) {
        let configs: Vec<crate::config::FixtureGroupConfig> = serde_yaml::from_str(yaml).unwrap();
        show.handle_meta_command(MetaCommand::Repatch(configs.into()))
            .unwrap();
    }

    #[test]
    fn repatch_grows_universes() {
        let mut show = show_from_yaml(ONE_UNIVERSE_PATCH);
        assert_eq!(show.dmx.len(), 1);

        repatch_from_yaml(&mut show, TWO_UNIVERSE_PATCH);
        assert_eq!(show.dmx.len(), 2);
    }

    #[test]
    fn repatch_shrinks_universes() {
        let mut show = show_from_yaml(TWO_UNIVERSE_PATCH);
        assert_eq!(show.dmx.len(), 2);

        repatch_from_yaml(&mut show, ONE_UNIVERSE_PATCH);
        assert_eq!(show.dmx.len(), 1);
    }

    #[test]
    fn snapshot_animation_state_skipped_when_inactive() {
        let mut show = show_from_yaml(ONE_UNIVERSE_PATCH);

        // Visualizer is inactive by default.
        assert!(
            !show
                .gui_state
                .visualizer_active
                .load(std::sync::atomic::Ordering::Relaxed)
        );

        show.snapshot_animation_state().unwrap();

        // Animation state should still be the default (fixture_count == 0).
        let state = show.gui_state.animation_state.load();
        assert_eq!(state.fixture_count, 0);
    }

    #[test]
    fn snapshot_animation_state_updates_when_active() {
        let mut show = show_from_yaml(ONE_UNIVERSE_PATCH);

        // Activate the visualizer.
        show.gui_state
            .visualizer_active
            .store(true, std::sync::atomic::Ordering::Relaxed);

        show.snapshot_animation_state().unwrap();

        // The patch has one Dimmer fixture, so fixture_count should be 1.
        let state = show.gui_state.animation_state.load();
        assert_eq!(state.fixture_count, 1);
    }

    /// Generate YAML for N dimmer fixtures, each on a separate channel.
    fn n_dimmer_yaml(n: usize) -> String {
        (0..n)
            .map(|i| {
                format!(
                    "- fixture: Dimmer\n  group: dimmer_{i}\n  patches:\n    - addr: {}\n",
                    i + 1
                )
            })
            .collect()
    }

    #[test]
    fn master_strobe_channel_routing() {
        // 1 dimmer = 1 channel, 1 wing, strobe on fader 7.
        let mut show = show_from_yaml(ONE_UNIVERSE_PATCH);
        let strobe_ch = strobe_control_channel(show.patch.channel_count());
        assert_eq!(strobe_ch, 7);

        // Default strobe intensity is 1.0 (full).
        assert_eq!(
            show.master_controls.strobe().intensity(),
            number::UnipolarFloat::ONE
        );

        // Disabled by default: level to strobe fader goes to fixture, not strobe.
        show.handle_channel_message(&crate::channel::ControlMessage::Control {
            channel_id: Some(strobe_ch),
            msg: crate::channel::ChannelControlMessage::Level(number::UnipolarFloat::new(0.5)),
        })
        // Channel 7 has no fixture group, so this is an out-of-range error.
        .ok();
        // Intensity unchanged — message didn't route to strobe.
        assert_eq!(
            show.master_controls.strobe().intensity(),
            number::UnipolarFloat::ONE
        );

        // Enable strobe.
        show.handle_meta_command(MetaCommand::SetMasterStrobeChannel(true))
            .unwrap();
        assert_eq!(show.master_strobe_channel, Some(7));

        // Level routes to strobe intensity.
        show.handle_channel_message(&crate::channel::ControlMessage::Control {
            channel_id: Some(strobe_ch),
            msg: crate::channel::ChannelControlMessage::Level(number::UnipolarFloat::new(0.75)),
        })
        .unwrap();
        assert_eq!(
            show.master_controls.strobe().intensity(),
            number::UnipolarFloat::new(0.75)
        );

        // Knob 0 routes to strobe rate.
        let initial_rate = show.master_controls.strobe().rate_control();
        show.handle_channel_message(&crate::channel::ControlMessage::Control {
            channel_id: Some(strobe_ch),
            msg: crate::channel::ChannelControlMessage::Knob {
                index: 0,
                value: crate::channel::KnobValue::Unipolar(number::UnipolarFloat::new(0.5)),
            },
        })
        .unwrap();
        assert_ne!(show.master_controls.strobe().rate_control(), initial_rate);

        // Non-strobe fader (channel 0) doesn't affect strobe.
        let intensity_before = show.master_controls.strobe().intensity();
        show.handle_channel_message(&crate::channel::ControlMessage::Control {
            channel_id: Some(0),
            msg: crate::channel::ChannelControlMessage::Level(number::UnipolarFloat::new(1.0)),
        })
        .unwrap();
        assert_eq!(show.master_controls.strobe().intensity(), intensity_before);
    }

    #[test]
    fn enable_strobe_fails_when_fader_occupied() {
        // 8 dimmers = all 8 faders on wing 1 occupied.
        let yaml = n_dimmer_yaml(8);
        let mut show = show_from_yaml(&yaml);
        assert_eq!(show.patch.channel_count(), 8);

        let result = show.handle_meta_command(MetaCommand::SetMasterStrobeChannel(true));
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("occupied"));
    }

    #[test]
    fn repatch_auto_disables_strobe_when_fader_occupied() {
        // 7 dimmers = fader 7 available for strobe.
        let yaml = n_dimmer_yaml(7);
        let mut show = show_from_yaml(&yaml);

        show.handle_meta_command(MetaCommand::SetMasterStrobeChannel(true))
            .unwrap();
        assert_eq!(show.master_strobe_channel, Some(7));

        // Repatch to 8 dimmers — fader 7 now occupied.
        let yaml_8 = n_dimmer_yaml(8);
        repatch_from_yaml(&mut show, &yaml_8);

        assert_eq!(show.master_strobe_channel, None);
        assert!(
            !show
                .gui_state
                .master_strobe_fader_channel_mapped
                .load(std::sync::atomic::Ordering::Relaxed)
        );
    }

    #[test]
    fn repatch_moves_strobe_to_new_wing() {
        // 7 dimmers = 1 wing, strobe on fader 7.
        let yaml = n_dimmer_yaml(7);
        let mut show = show_from_yaml(&yaml);

        show.handle_meta_command(MetaCommand::SetMasterStrobeChannel(true))
            .unwrap();
        assert_eq!(show.master_strobe_channel, Some(7));

        // Repatch to 9 dimmers — 2 wings, strobe moves to fader 15.
        let yaml_9 = n_dimmer_yaml(9);
        repatch_from_yaml(&mut show, &yaml_9);

        assert_eq!(show.master_strobe_channel, Some(15));
        assert!(
            show.gui_state
                .master_strobe_fader_channel_mapped
                .load(std::sync::atomic::Ordering::Relaxed)
        );
    }

    // === Positioner end-to-end integration tests ============================
    //
    // These exercise the full OSC dispatch path on Show: fire a message at
    // `handle_osc_message`, then assert both the in-memory mutation and the
    // OSC responses that came back out. The two-iPad binding choreography
    // (Positioner tab ↔ per-group preset selector) is the canonical case —
    // see the docstring on each test for the scenario it covers.
    mod positioner_integration {
        use super::*;
        use crate::positioner::BumpStep;

        /// One IWashLed group on channel 0.
        const ONE_IWASH: &str = "\
- fixture: IWashLed
  patches:
    - addr: 1
    - addr: 13
";

        /// Two IWashLed groups: IWashFront on channel 0, IWashBack on channel 1.
        const TWO_IWASH: &str = "\
- fixture: IWashLed
  group: IWashFront
  patches:
    - addr: 1
    - addr: 13
- fixture: IWashLed
  group: IWashBack
  patches:
    - addr: 25
    - addr: 37
";

        /// IWashLed on channel 0 (positionable) and Dimmer on channel 1
        /// (non-positionable).
        const MIXED_POSITIONABLE_AND_NOT: &str = "\
- fixture: IWashLed
  patches:
    - addr: 1
- fixture: Dimmer
  patches:
    - addr: 100
";

        /// Tap `/Positioner/Preset/1/3` on the Positioner tab while
        /// IWashLed is the current channel. Expect: in-memory `active`
        /// flips to slot 2, the per-group `PositionPresetSelect/1/3` echoes
        /// back, and both label arrays are pushed.
        #[test]
        fn positioner_preset_tap_echoes_to_per_group_radio() {
            let (mut show, capture, _send) = show_with_capture_from_yaml(ONE_IWASH);
            // Discard the initial sync emits from Show::test_new construction.
            capture.drain();

            fire_press(&mut show, "/Positioner/Preset/1/3").unwrap();
            let emits = capture.drain_by_addr();

            // In-memory state: active slot is 2 (0-indexed; 3rd button).
            let channel = show.channels_for_test().current_channel().unwrap();
            let positioner = show
                .patch_for_test()
                .channel_group(channel)
                .unwrap()
                .positioner()
                .expect("IWashLed is positionable");
            assert_eq!(positioner.active(), 2);

            // Positioner-tab Preset radio: slot 3 lit, others dark.
            for i in 1..=8 {
                let addr = format!("/Positioner/Preset/1/{i}");
                let expected = if i == 3 {
                    OscType::Float(1.0)
                } else {
                    OscType::Float(0.0)
                };
                assert_eq!(
                    emits.get(&addr),
                    Some(&expected),
                    "/Positioner/Preset slot {i} state wrong",
                );
            }

            // Per-group PositionPresetSelect: slot 3 lit, others dark.
            for i in 1..=8 {
                let addr = format!("/IWashLed/PositionPresetSelect/1/{i}");
                let expected = if i == 3 {
                    OscType::Float(1.0)
                } else {
                    OscType::Float(0.0)
                };
                assert_eq!(
                    emits.get(&addr),
                    Some(&expected),
                    "/IWashLed/PositionPresetSelect slot {i} state wrong",
                );
            }

            // Both label arrays emitted with the default preset names.
            for i in 0..8 {
                let expected = OscType::String(format!("Position {}", i + 1));
                assert_eq!(
                    emits.get(&format!("/Positioner/PresetLabel/{i}")),
                    Some(&expected),
                );
                assert_eq!(
                    emits.get(&format!("/IWashLed/PositionPresetLabel/{i}")),
                    Some(&expected),
                );
            }
        }

        /// Tap `/IWashBack/PositionPresetSelect/1/5` while IWashFront IS the
        /// current channel. Expect: IWashBack's state mutates, IWashFront's
        /// doesn't, and the Positioner tab is *not*
        /// touched (operator on the iWashFront tab shouldn't see slot 5 light
        /// up because of activity on a different group).
        #[test]
        fn per_group_preset_tap_on_other_channel_is_silent_on_channel_tab() {
            let (mut show, capture, _send) = show_with_capture_from_yaml(TWO_IWASH);
            // IWashFront is patched first and becomes the default current channel.
            let front_id = show.channels_for_test().current_channel().unwrap();
            capture.drain();

            fire_press(&mut show, "/IWashBack/PositionPresetSelect/1/5").unwrap();
            let emits = capture.drain_by_addr();

            // IWashBack's state mutated; IWashFront's untouched.
            let front_active = show
                .patch_for_test()
                .channel_group(front_id)
                .unwrap()
                .positioner()
                .unwrap()
                .active();
            assert_eq!(front_active, 0, "IWashFront active unchanged");

            // Look up IWashBack by name (it's on channel 1).
            let back_channel = crate::fixture::patch::ChannelId::for_test(1);
            let back_active = show
                .patch_for_test()
                .channel_group(back_channel)
                .unwrap()
                .positioner()
                .unwrap()
                .active();
            assert_eq!(back_active, 4, "IWashBack active flipped");

            // Per-group radio echoed on IWashBack.
            assert_eq!(
                emits.get("/IWashBack/PositionPresetSelect/1/5"),
                Some(&OscType::Float(1.0)),
            );
            // Positioner tab was NOT touched — no Preset radio echo, no
            // label refresh. This is the key cross-binding-isolation check.
            for i in 1..=8 {
                let addr = format!("/Positioner/Preset/1/{i}");
                assert!(
                    !emits.contains_key(&addr),
                    "unexpected Positioner-tab emit at {addr}: {:?}",
                    emits.get(&addr),
                );
            }
            for i in 0..8 {
                let addr = format!("/Positioner/PresetLabel/{i}");
                assert!(
                    !emits.contains_key(&addr),
                    "unexpected Positioner-tab emit at {addr}: {:?}",
                    emits.get(&addr),
                );
            }
        }

        /// Switch from a positionable channel to a non-positionable one. The
        /// Positioner tab should clear: FixtureLabel
        /// reads `"—"`, faders snap to 0, preset radio fully deselects,
        /// preset labels go blank.
        #[test]
        fn channel_switch_to_non_positionable_clears_tab() {
            let (mut show, capture, _send) =
                show_with_capture_from_yaml(MIXED_POSITIONABLE_AND_NOT);
            // Default current channel is the IWashLed (channel 0); switch to
            // the Dimmer (channel 1).
            capture.drain();
            fire_press(&mut show, "/Show/Channel/1/2").unwrap();

            let emits = capture.drain_by_addr();

            // FixtureLabel cleared.
            assert_eq!(
                emits.get("/Positioner/FixtureLabel"),
                Some(&OscType::String("—".to_string())),
            );
            // Faders zeroed.
            assert_eq!(emits.get("/Positioner/X"), Some(&OscType::Float(0.0)));
            assert_eq!(emits.get("/Positioner/Y"), Some(&OscType::Float(0.0)));
            assert_eq!(emits.get("/Positioner/Focus"), Some(&OscType::Float(0.0)));
            // Preset radio fully deselected (all 8 buttons emit 0.0).
            for i in 1..=8 {
                let addr = format!("/Positioner/Preset/1/{i}");
                assert_eq!(
                    emits.get(&addr),
                    Some(&OscType::Float(0.0)),
                    "preset slot {i} not cleared",
                );
            }
            // Preset labels blanked (empty strings).
            for i in 0..8 {
                let addr = format!("/Positioner/PresetLabel/{i}");
                assert_eq!(
                    emits.get(&addr),
                    Some(&OscType::String(String::new())),
                    "preset label {i} not blanked",
                );
            }
        }

        /// `MetaCommand::RenamePositionerPreset` should update the active
        /// preset's name and re-emit both label arrays (Positioner-tab and
        /// per-group). This is the desktop GUI's rename flow end-to-end.
        #[test]
        fn rename_emits_to_both_label_arrays() {
            let (mut show, capture, _send) = show_with_capture_from_yaml(ONE_IWASH);
            capture.drain();

            show.handle_meta_command(MetaCommand::RenamePositionerPreset("Bar Spots".to_string()))
                .unwrap();

            let emits = capture.drain_by_addr();

            // In-memory state: slot 0 (the default active slot) was renamed.
            let channel = show.channels_for_test().current_channel().unwrap();
            let positioner = show
                .patch_for_test()
                .channel_group(channel)
                .unwrap()
                .positioner()
                .unwrap();
            assert_eq!(positioner.presets()[0].name, "Bar Spots");

            // Both label-array slot 0 entries reflect the new name.
            let expected = OscType::String("Bar Spots".to_string());
            assert_eq!(
                emits.get("/Positioner/PresetLabel/0"),
                Some(&expected),
                "Positioner-tab label not updated",
            );
            assert_eq!(
                emits.get("/IWashLed/PositionPresetLabel/0"),
                Some(&expected),
                "per-group label not updated",
            );
        }

        /// End-to-end smoke test of the fader → in-memory state path: write
        /// `/Positioner/X` and assert the offset stored in the active preset.
        /// Then bump that offset via the momentary bump button and assert
        /// the magnitude added. Covers `selected_fixture` lookup, fader-to-
        /// offset write, and bump's modify-in-place arithmetic.
        #[test]
        fn fader_and_bump_persist_offset_into_in_memory_state() {
            let (mut show, capture, _send) = show_with_capture_from_yaml(ONE_IWASH);
            capture.drain();

            // Slide /Positioner/X to 0.5.
            fire(&mut show, "/Positioner/X", OscType::Float(0.5)).unwrap();
            capture.drain();

            let channel = show.channels_for_test().current_channel().unwrap();
            let x = show
                .patch_for_test()
                .channel_group(channel)
                .unwrap()
                .positioner()
                .unwrap()
                .presets()[0]
                .offsets[0]
                .x
                .val();
            assert!((x - 0.5).abs() < 1e-9, "X fader didn't land: x = {x}");

            // Bump up — default step is Medium (0.01).
            fire_press(&mut show, "/Positioner/XBumpUp").unwrap();
            let x = show
                .patch_for_test()
                .channel_group(channel)
                .unwrap()
                .positioner()
                .unwrap()
                .presets()[0]
                .offsets[0]
                .x
                .val();
            let expected = 0.5 + BumpStep::Medium.magnitude();
            assert!(
                (x - expected).abs() < 1e-9,
                "bump didn't accumulate: x = {x}, expected {expected}",
            );
        }

        /// All Positioner-tab control addresses, used to assert that a given
        /// command emits to *only* the expected subset.
        fn all_positioner_tab_addrs() -> Vec<String> {
            let mut addrs = vec![
                "/Positioner/X".to_string(),
                "/Positioner/Y".to_string(),
                "/Positioner/Focus".to_string(),
                "/Positioner/FixtureLabel".to_string(),
            ];
            for i in 1..=8 {
                addrs.push(format!("/Positioner/Preset/1/{i}"));
            }
            for i in 0..8 {
                addrs.push(format!("/Positioner/PresetLabel/{i}"));
            }
            for i in 1..=3 {
                addrs.push(format!("/Positioner/BumpStep/1/{i}"));
            }
            addrs
        }

        /// Assert `emits` includes exactly the addresses in `expected` from
        /// the set of all Positioner-tab addresses. Other addresses outside
        /// that set (per-group, channel state, etc.) are ignored.
        fn assert_positioner_tab_emits(emits: &HashMap<String, OscType>, expected: &[&str]) {
            let expected: std::collections::HashSet<&str> = expected.iter().copied().collect();
            for addr in all_positioner_tab_addrs() {
                let present = emits.contains_key(&addr);
                let want = expected.contains(addr.as_str());
                assert_eq!(
                    present, want,
                    "Positioner-tab addr {addr}: present={present}, expected={want}",
                );
            }
        }

        /// Moving one fader must push only that fader, not the other two
        /// axes, not FixtureLabel, not the preset radio, not bump-step.
        #[test]
        fn fader_emits_only_target_axis() {
            let (mut show, capture, _send) = show_with_capture_from_yaml(ONE_IWASH);
            capture.drain();

            fire(&mut show, "/Positioner/X", OscType::Float(0.5)).unwrap();
            let emits = capture.drain_by_addr();
            assert_positioner_tab_emits(&emits, &["/Positioner/X"]);

            fire(&mut show, "/Positioner/Y", OscType::Float(-0.25)).unwrap();
            let emits = capture.drain_by_addr();
            assert_positioner_tab_emits(&emits, &["/Positioner/Y"]);

            fire(&mut show, "/Positioner/Focus", OscType::Float(0.1)).unwrap();
            let emits = capture.drain_by_addr();
            assert_positioner_tab_emits(&emits, &["/Positioner/Focus"]);
        }

        /// Renaming a preset slot must push only that one slot's label on
        /// each surface — not the other 7, not the preset radio, not the
        /// faders, not anything else on the Positioner tab.
        #[test]
        fn rename_emits_only_active_slot_label() {
            let (mut show, capture, _send) = show_with_capture_from_yaml(ONE_IWASH);
            capture.drain();

            show.handle_meta_command(MetaCommand::RenamePositionerPreset("Bar Spots".to_string()))
                .unwrap();
            let emits = capture.drain_by_addr();

            // On the Positioner tab: only PresetLabel/0 was emitted.
            assert_positioner_tab_emits(&emits, &["/Positioner/PresetLabel/0"]);

            // On the per-group surface: only PositionPresetLabel/0 was
            // emitted; the other 7 label slots and the preset radio were not.
            assert!(emits.contains_key("/IWashLed/PositionPresetLabel/0"));
            for i in 1..8 {
                let addr = format!("/IWashLed/PositionPresetLabel/{i}");
                assert!(
                    !emits.contains_key(&addr),
                    "unexpected per-group label emit at {addr}: {:?}",
                    emits.get(&addr),
                );
            }
            for i in 1..=8 {
                let addr = format!("/IWashLed/PositionPresetSelect/1/{i}");
                assert!(
                    !emits.contains_key(&addr),
                    "unexpected per-group radio emit at {addr}: {:?}",
                    emits.get(&addr),
                );
            }
        }
    }
}
