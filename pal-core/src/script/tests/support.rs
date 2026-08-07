use super::*;

pub(super) fn table(entries: &[[u16; 4]]) -> ScriptTable {
    let data = entries
        .iter()
        .flat_map(|entry| entry.iter().flat_map(|value| value.to_le_bytes()))
        .collect::<Vec<_>>();
    ScriptTable::parse(&data).unwrap()
}

pub(super) fn trigger(entry: u16) -> TriggerRequest {
    TriggerRequest {
        object_id: 7,
        script_entry: entry,
        kind: TriggerKind::Touch,
    }
}
