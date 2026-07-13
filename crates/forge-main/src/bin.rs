use calloop::{EventLoop, PostAction};
use calloop_wayland_source::WaylandSource;
use wayland_client::{Connection, EventQueue};
struct State;
fn main() {
    let conn = match Connection::connect_to_env() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Failed to connect to Wayland display. Are you running under a Wayland compositor?\nError: {}", e);
            std::process::exit(1);
        }
    };
    let queue: EventQueue<State> = conn.new_event_queue();
    let mut event_loop: EventLoop<State> = EventLoop::try_new().unwrap();
    let source = WaylandSource::new(conn.clone(), queue);
    event_loop.handle().insert_source(source, |(), queue, state| {
        queue.dispatch_pending(state)
    }).unwrap();
}
