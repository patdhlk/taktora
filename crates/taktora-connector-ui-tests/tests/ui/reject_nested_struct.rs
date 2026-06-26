use taktora_connector_ui::ViewModel;

// A nested plain struct is part of REQ_0858's type set but is deferred: the
// derive classifies any non-scalar/non-array/non-`BoundedString` field as a
// C-like enum, so this fails with `ImageEnum`'s purposeful diagnostic.
struct Nested {
    x: f64,
}

#[derive(ViewModel)]
struct Bad {
    nested: Nested,
}

fn main() {}
