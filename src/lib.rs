pub mod common;
pub mod configuration;
pub mod proxy;
pub mod selection_method;

pub mod prelude {
    pub use crate::configuration::Settings;
    pub use crate::proxy::Server;
}
