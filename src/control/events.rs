#[cfg(not(feature = "bazel"))]
pub mod proto {
    tonic::include_proto!("talon.events");
}

#[cfg(feature = "bazel")]
pub mod proto {
    pub use talon_events_proto::talon::events::*;
}

pub use proto::*;
