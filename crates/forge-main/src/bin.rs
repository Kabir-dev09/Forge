use calloop::{EventLoop, PostAction};
use calloop_wayland_source::WaylandSource;
use wayland_client::{Connection, EventQueue};
struct State;
fn main() {
    let conn = Connection::connect_to_env().unwrap();
    let queue: EventQueue<State> = conn.new_event_queue();
    let mut event_loop: EventLoop<State> = EventLoop::try_new().unwrap();
    let source = WaylandSource::new(conn.clone(), queue);
    event_loop.handle().insert_source(source, |(), queue, state| {
        queue.dispatch_pending(state)
    }).unwrap();
}
