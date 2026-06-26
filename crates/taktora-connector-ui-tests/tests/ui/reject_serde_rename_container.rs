use serde::Serialize;
use taktora_connector_ui::ViewModel;

#[derive(Serialize, ViewModel)]
#[serde(rename_all = "camelCase")]
struct Bad {
    field_one: bool,
}

fn main() {}
