//! Accept calls without replying, including ReadState. A caller may select any
//! deployed asset program, so ATA must not commit a pending conversation here.
use lee_core::program::{ReceiveInput, Response};

lee_core::actor_entrypoint!(raw handle_message);

fn handle_message(_: &ReceiveInput) -> Response {
    Response::keep_state()
}
