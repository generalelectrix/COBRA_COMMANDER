use crate::config::{FixtureGroupConfig, PatchBlock};
use crate::fixture::patch::Patcher;
use crate::gui_state::PatchSnapshot;

pub(crate) struct WorkingGroup {
    pub config: FixtureGroupConfig,
    /// Channel count per patch block, resolved at creation via
    /// patcher.create_patch(). One entry per PatchBlock in config.patches.
    pub channel_counts: Vec<usize>,
}

pub(crate) struct PatchWorkingCopy {
    pub groups: Vec<WorkingGroup>,
}

impl PatchWorkingCopy {
    pub fn from_snapshot(snapshot: &PatchSnapshot, patchers: &[Patcher]) -> Self {
        let groups = snapshot
            .groups
            .iter()
            .map(|group_cfg| Self::resolve_group(group_cfg, patchers))
            .collect();
        Self { groups }
    }

    pub fn resolve_group(group_cfg: &FixtureGroupConfig, patchers: &[Patcher]) -> WorkingGroup {
        let patcher = patchers.iter().find(|p| p.name.0 == group_cfg.fixture);
        let channel_counts = group_cfg
            .patches
            .iter()
            .map(|block| resolve_channel_count(patcher, group_cfg, block))
            .collect();
        WorkingGroup {
            config: group_cfg.clone(),
            channel_counts,
        }
    }

    pub fn configs(&self) -> Vec<FixtureGroupConfig> {
        self.groups.iter().map(|g| g.config.clone()).collect()
    }
}

fn resolve_channel_count(
    patcher: Option<&Patcher>,
    group_cfg: &FixtureGroupConfig,
    block: &PatchBlock,
) -> usize {
    patcher
        .and_then(|p| {
            (p.create_patch)(group_cfg.options.clone(), block.options.clone())
                .ok()
                .map(|cfg| cfg.channel_count)
        })
        .unwrap_or(0)
}
