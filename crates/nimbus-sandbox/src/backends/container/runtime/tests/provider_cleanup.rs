//! Container provider-cleanup and durable authority-ordering proofs.

use super::*;

#[path = "provider_cleanup/assertions.rs"]
mod assertions;
#[path = "provider_cleanup/execution_context.rs"]
mod execution_context;
#[path = "provider_cleanup/forwarder_observer.rs"]
mod forwarder_observer;
#[path = "provider_cleanup/machine_publication.rs"]
mod machine_publication;
#[path = "provider_cleanup/netavark_restart.rs"]
mod netavark_restart;
#[path = "provider_cleanup/network_finality.rs"]
mod network_finality;
#[path = "provider_cleanup/startup_fencing.rs"]
mod startup_fencing;

use assertions::*;
