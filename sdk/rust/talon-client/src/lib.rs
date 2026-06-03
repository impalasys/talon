pub mod generated {
    pub mod talon {
        pub mod config {
            include!("generated/talon.config.rs");
        }
        pub mod manifests {
            include!("generated/talon.manifests.rs");
        }
        pub mod models {
            include!("generated/talon.models.rs");
        }
        pub mod events {
            include!("generated/talon.events.rs");
        }
        pub mod gateway {
            include!("generated/talon.gateway.rs");
        }
    }
}

pub use generated::talon::*;

