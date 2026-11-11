//! The port of `tests/queues-tests.py`: the queue of the phone, its dispatcher and its commands, with a made-up phone.
//! Nothing here touches a device, a real chat or the real queue: the state lives in a temporary folder and the
//! dispatcher is stepped by hand. `support` is the Python's `QueueTestCase` and `FakeDevice`; each other file is a
//! group of the Python's test classes, named after them.

mod support;
mod the_queue;
mod blocked_and_actions;
mod on_the_mac_too;
mod probes_and_terminal;
