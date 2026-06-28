```
                                #@%         #@@,                               
                              ,@@@@@@%%,#%@@@@@@@                              
                              %@@ *@@,   ,%@@ &@@                              
                              @@@ @*       ,@,&@@.                             
                         #@@@@@@@#%%       *@#@@@@@@@#                         
                    *@@@@@@@@@@@@@@@       @@@@@@@@@@@@@@@#                    
                *@@@@@@@@@@@@@@@,#@,%     ,#@@ @@@@@@@@@@@@@@@#                
             &@@@@@@@@@@@@@@@@@@% @@       %@, @@@@@@@@@@@@@@@@@@@*            
          #@@@@@@@@@@@@@@@@@@@@&  *@*      @@  @@@@@@@@@@@@@@@@@@@@@@          
        @@@@@@@@@@@@@@@@@@%        @@@@@@@@@        #@@@@@@@@@@@@@@@@@@*       
     *@@@@@@@@@@@@@@@@%  #@@@@@@@,            @@@@@@@@, *@@@@@@@@@@@@@@@@#     
    @@@@@@@@@@@@@@@%  &@@@@@@@@@@              @@@@@@@@@@* #@@@@@@@@@@@@@@@    
  *@@@@@@@@@@@@@@# .,,,,,,,,,,,,               ,,,,,,,,,,,,, *@@@@@@@@@@@@@@&  
 #@@@@@@@@@@@@@@  @@@@@@@@@@@@@%               ,@@@@@@@@@@@@@, *@@@@@@@@@@@@@& 
,@@@@@@@@@@@@@&                                                  @@@@@@@@@@@@@%
@@@@@@@@@@@@@% #@@@@@@@@@@@@@@@                 *@@@@@@@@@@@@@@@ ,@@@@@@@@@@@@@
@@@@@@@@@@@@@ ,@@@@@@@@@@@@@@@%                  @@@@@@@@@@@@@@@% @@@@@@@@@@@@@
@@@@@@@@@@@@@ ,@@@@@@@@@@@@@@@%                  @@@@@@@@@@@@@@@@ %@@@@@@@@@@@@
@@@@@@@@@@@@@  @@@@@@@@@@@@@@@%                  @@@@@@@@@@@@@@@@ %@@@@@@@@@@@@
@@@@@@@@@@@@@,                                                    @@@@@@@@@@@@@
#@@@@@@@@@@@@@* @@@@@@@@@@@@@@%                  @@@@@@@@@@@@@@  @@@@@@@@@@@@@@
 %@@@@@@@@@@@@@# ,%%%%%%%%%%%%%                 ,%%%%%%%%%%%%#  @@@@@@@@@@@@@% 
   @@@@@@@@@@@@@@. #@@@@@@@@@@@                 %@@@@@@@@@@@  @@@@@@@@@@@@@@#  
    #@@@@@@@@@@@@@@* .@@@@@@@@@/                @@@@@@@@@* .@@@@@@@@@@@@@@@    
      *@@@@@@@@@@@@@@@,  ,,,,,,,               ,,,,,,,   #@@@@@@@@@@@@@@@      
         &@@@@@@@@@@@@@@@  #@@@@&             ,@@@@%  &@@@@@@@@@@@@@@@,        
            *@@@@@@@@@@@@@@%  .,,             ,,,  #@@@@@@@@@@@@@@&            
                *@@@@@@@@@@@@@# ,@,          @@  @@@@@@@@@@@@@@                
                    ,@@@@@@@@@@@, %         %  @@@@@@@@@@@%                    
                         #@@@@@@@#           #@@@@@@@@*                        
                             #@@@@@         #@@@@@*                            
                                #@@#       *@@@,                               
                                  #@,      @@                                  
                                   *@     ,#                                   
```

# Cobra Commander

A live performance lighting controller. The host is a computer running the app
(desktop GUI via `eframe`/`egui`). Every hardware interface below is optional
and independently enableable. With nothing attached the app still runs in
offline/mock mode.

## Hardware setup

### DMX output (the core, drives fixtures)

DMX output is abstracted by [`rust_dmx`]. Three paths:

- **USB-DMX:** an Enttec-style FTDI serial dongle (the Enttec runs ~40 fps).
- **Art-Net:** a network DMX gateway. The DMX config panel can scan for nodes.
- **Offline:** a mock port, no hardware.

Fixtures are any DMX-addressable units matching one of the built-in profiles in
`src/fixture/profile/` (washes, moving heads, lasers, strobes, and more). Patch
them by universe and address in a `.cobra` show file (YAML).

Minimum to put light on stage: one USB-DMX dongle (or an Art-Net node) plus DMX
fixtures.

### MIDI control surfaces

Physical knobs, faders, and buttons over USB MIDI. Plug in one or several of the
supported devices (`src/midi/mod.rs`):

- Akai APC20
- Novation Launch Control XL
- Behringer CMD MM1
- Behringer CMD DV1
- Akai AMX
- Color Organ (audio-reactive)

Control is bidirectional: the app drives LEDs and motorized faders back.
Devices are hot-pluggable.

### OSC / TouchOSC

The app runs a UDP OSC listener and advertises itself over mDNS so a TouchOSC
client on the same LAN finds it automatically. Run TouchOSC on a tablet or
phone on the same network and load the layouts in `touchosc/`. Needs nothing but
a shared network.

### Audio input (beat / envelope sync)

In local ("Internal") clock mode the app opens a named system audio device and
runs envelope analysis to drive clocks and the color organ. Feed line or mic
into any system audio interface, then select that device in the audio panel. No
special hardware beyond what the OS exposes.

### Remote clock / tempo (optional)

As an alternative to local audio clocking, the app can subscribe to a remote
clock and audio-envelope provider discovered over the network (zeroconf). This
needs another box on the LAN publishing the clock; no local audio hardware is
required in this mode.

### Network fixtures (WLED)

WLED nodes are controlled over HTTP JSON by URL, patched in the show file like
any other fixture. The node just needs to be reachable on the LAN.

## "All features" bench setup

1. Computer running the app.
2. One USB-DMX dongle (Enttec) **or** an Art-Net node, plus your DMX fixtures.
3. One or more of the supported USB-MIDI surfaces.
4. A tablet or phone on the same LAN running TouchOSC.
5. An audio feed into the computer's sound input (for beat/envelope and the
   color organ) **or** a network clock provider instead.
6. Optional WLED node(s) on the LAN.
7. A wired LAN/switch tying together Art-Net, OSC/mDNS, WLED, and the remote
   clock.

Two "either/or" choices worth calling out: Art-Net vs USB for DMX, and local
audio vs a remote clock service for tempo. You do not need both halves of
either.

[`rust_dmx`]: https://github.com/generalelectrix/rust_dmx
