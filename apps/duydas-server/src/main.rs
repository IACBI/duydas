//! Duydaş server (Phase 1): auth, gateway, REST, voice SFU and media modules in one binary.

fn main() {
    // Operator-facing CLI output is English-only by design; end-user text is localized on clients.
    eprintln!(
        "duydas-server {} (protocol v{}): not implemented yet, see docs/architecture.md",
        env!("CARGO_PKG_VERSION"),
        duydas_proto::PROTOCOL_VERSION
    );
}
