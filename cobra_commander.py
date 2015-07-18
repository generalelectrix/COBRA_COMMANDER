from __future__ import print_function

import sys
import yaml
import OSC
import logging
import controls

from multiprocessing import Process, Queue
from Queue import Empty


class OSCController(object):
    """Class to manage oversight of an external OSC control surface."""
    def __init__(self, config, control_queue):
        self.receiver = OSC.OSCServer( (config['receive_host'], config['receive_port']) )
        self.receiver.addMsgHandler('default', self.handle_osc_message)

        # use a closure to pass messages back to this instance
        #def handle_osc_message(addr, type_tags, payload, source_addr):
        #    self.handle_osc_message(addr, type_tags, payload, source_addr)
        #self.receiver.addMsgHandler('default', handle_osc_message)

        self.sender = OSC.OSCClient()
        self.sender.connect( (config['send_host'], config['send_port']) )
        self.control_groups = {}

        self.control_queue = control_queue

    def create_control_group(self, name):
        if name not in self.control_groups:
            self.control_groups[name] = {}

    def create_simple_control(self, group, name, comet_control, preprocessor=None):
        """Create a pure osc listener, with no talkback."""
        if preprocessor is None:
            def callback(_, payload):
                self.send_comet_control(comet_control, payload)
        else:
            def callback(_, payload):
                processed = preprocessor(payload)
                self.send_comet_control(comet_control, processed)

        self.control_groups[group][name] = callback

    def create_radio_button_control(self, group, name, shape, comet_control):
        """Create a radio button array control.

        This has been special-cased for present purposes.
        """
        def callback(addr, payload):
            elements = addr.split('/')
            group_name = elements[1]
            control_name = elements[2]
            base_addr = '/' + group_name + '/' + control_name + '/{}/{}'
            x = int(elements[3])
            y = int(elements[4])
            for x_but in xrange(shape[0]):
                for y_but in xrange(shape[1]):
                    this_addr = base_addr.format(x_but+1, y_but+1)
                    if x_but+1 == x and y_but+1 == y:
                        self.send_button_on(this_addr)
                    else:
                        self.send_button_off(this_addr)
            self.send_comet_control(comet_control, x-1)
        self.control_groups[group][name] = callback


    def handle_osc_message(self, addr, type_tags, payload, source_addr):
        elements = addr.split('/')
        if len(elements) < 3:
            return
        group_name = elements[1]
        control_name = elements[2]
        try:
            group = self.control_groups[group_name]
        except KeyError:
            logging.log("Unknown control group: {}".format(group_name))
            return
        try:
            control = group[control_name]
        except KeyError:
            logging.log("Unknown control {} in group {}"
                        .format(control_name, group_name))
        control(addr, payload[0])

    def send_comet_control(self, control, value):
        self.control_queue.put((control, value))

    def send_button_on(self, addr):
        msg = OSC.OSCMessage()
        msg.setAddress(addr)
        msg.append(1.0)
        self.sender.send(msg)

    def send_button_off(self, addr):
        msg = OSC.OSCMessage()
        msg.setAddress(addr)
        msg.append(0.0)
        self.sender.send(msg)

def unpack(val_list):
    """Decorator to unpack only first arg of touchOSC messages."""
    def wrapped_callback(callback):
        callback(val_list[0])
    return wrapped_callback

def ignore_all_but_1(value):
    return value if value == 1.0 else None

def quadratic_fader(value):
    return value**2

def quartic_fader(value):
    return value**4

def setup_controls(cont):

    # make groups
    cont.create_control_group('Controls')
    cont.create_control_group('Music')
    cont.create_control_group('Debug')

    # add controls
    cont.create_simple_control('Controls', 'Shutter', controls.Shutter)
    cont.create_simple_control('Controls', 'Strobe', controls.Strobe)
    cont.create_simple_control('Controls', 'StrobeRate', controls.StrobeRate, quartic_fader)
    cont.create_simple_control('Controls', 'Mspeed', controls.Mspeed)
    cont.create_simple_control('Controls', 'AutoStep', controls.AutoStep)
    cont.create_simple_control('Controls', 'AutoStepRate', controls.AutoStepRate)

    cont.create_simple_control('Controls', 'StepBackwards', controls.StepBackwards, ignore_all_but_1)
    cont.create_simple_control('Controls', 'StepForwards', controls.StepForwards, ignore_all_but_1)

    cont.create_radio_button_control('Controls', 'SelectMacro', (10,1), controls.SelectMacro)

    cont.create_simple_control('Music', 'ShutterSoundActive', controls.ShutterSoundActive)
    cont.create_simple_control('Music', 'TrigSoundActive', controls.TrigSoundActive)

    cont.create_simple_control('Debug', 'Reset', controls.Reset)


if __name__ == '__main__':
    # fire it up!

    import os
    import pyenttec as dmx
    from backend import run_backend
    import time
    import threading
    import socket


    try:
        enttec = dmx.select_port()
    except dmx.EnttecPortOpenError as err:
        print(err)
        quit()

    control_queue = Queue()
    command_queue = Queue()
    debug_queue = Queue()

    # initialize control streams
    with open('config.yaml') as config_file:
        config = yaml.safe_load(config_file)

    config["receive host"] = socket.gethostbyname(socket.gethostname())
    print("Using local IP address {}".format(config["receive host"]))
    osc_controller = OSCController(config, control_queue)
    setup_controls(osc_controller)

    debug = config["debug"]

    backend = Process(target=run_backend,
                      args=(control_queue,
                            command_queue,
                            enttec,
                            config['dmx_addr']-1,
                            debug_queue,
                            debug))
    backend.start()

    # start the osc server
    # Start OSCServer
    print("\nStarting OSCServer.")
    st = threading.Thread( target = osc_controller.receiver.serve_forever )
    st.start()

try:
    while True:
        if debug:
            try:
                print(debug_queue.get(block=False))
            except Empty:
                time.sleep(0.1)
        else:
            user_input = raw_input('Enter q to quit.')
            if user_input == 'q':
                break


finally:
    command_queue.put('quit')
    print("\nClosing OSCServer.")
    osc_controller.receiver.close()
    print("Waiting for Server-thread to finish")
    st.join() ##!!!
    print("Done")




