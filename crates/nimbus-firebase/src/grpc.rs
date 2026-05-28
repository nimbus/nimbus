pub mod generated {
    #![allow(dead_code, missing_docs, clippy::all)]

    include!(concat!(env!("OUT_DIR"), "/firebase_grpc.rs"));
}

pub mod listen_stream;
pub mod unary;
pub mod write_stream;

pub use listen_stream::RetainedListenRegistry;
pub use write_stream::WriteStreamRegistry;
