use serde::Serialize;
use taktora_connector_ui::ViewModel;

// A nested plain struct is part of REQ_0858's type set but is deferred: the
// derive classifies any non-scalar/non-array/non-`BoundedString` field as a
// C-like enum, so this fails with `ImageEnum`'s purposeful diagnostic.
//
// `Serialize` is derived (as a real ViewModel author would) so the only errors
// are the `ImageEnum` rejection — not an incidental "Bad: Serialize" cascade
// from the generated JSON encoder.
#[derive(Serialize)]
struct Nested {
    x: f64,
}

#[derive(Serialize, ViewModel)]
struct Bad {
    nested: Nested,
}

fn main() {}
