"""Abstraction layer on top of the DMX interface to the RA venus."""

class Venus(object):

    def __init__(self, dmx_addr):
        """Create a new wrapper for a Venus.

        All controls are bipolar or unipolar floats.
        """
        self.dmx_addr = dmx_addr

        self.base_rotation = 0.0
        self.cradle_rotation = 0.0
        self.head_rotation = 0.0
        self.color_rotation = 0.0

    def render(self, dmx_univ):
        """Render this Comet into a DMX universe."""
        #TODO
        pass

"""
DMX profile Venus

Motor 1 is base motor
Motor 2 is crescent translate motor
Motor 3 is saucer off axis rotate motor
Motor 4 is color carousel

Motor direction is split at 127
Lamp on/off is split at 127 (high is on)

1 - Motor 1 Dir
2 - Motor 1 Speed
3 - Motor 2 Dir
4 - Motor 2 Speed
5 - Motor 3 Dir
6 - Motor 3 Speed
7 - Motor 4 Dir
8 - Motor 4 Speed
9 - Lamp Control
"""

# controls and control actions

(BaseRotation,
 CradleRotation,
 HeadRotation,
 ColorRotation) = range(4)

# control actions
def base_rotation(venus, speed):
    venus.base_rotation = speed

def cradle_rotation(venus, speed):
    venus.cradle_rotation = speed

def head_rotation(venus, speed):
    venus.head_rotation = speed

def color_rotation(venus, speed):
    venus.color_rotation = speed

# control mapping
control_map = {
    BaseRotation: base_rotation,
    CradleRotation: cradle_rotation,
    HeadRotation: head_rotation,
    ColorRotation: color_rotation,}

def setup_controls(cont):

    # make groups
    cont.create_control_group('Controls')
    cont.create_control_group('Debug')

    # add controls
    cont.create_simple_control('Controls', 'BaseRotation', BaseRotation)
    cont.create_simple_control('Controls', 'CradleRotation', CradleRotation)
    cont.create_simple_control('Controls', 'HeadRotation', HeadRotation)
    cont.create_simple_control('Controls', 'ColorRotation', ColorRotation)

