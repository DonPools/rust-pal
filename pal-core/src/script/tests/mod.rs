use super::*;
use crate::battle::{BattleRequest, BattleResult};
use crate::role::Direction;
use crate::scene::{TriggerKind, TriggerRequest};
use pal_assets::script::ScriptTable;

use support::{table, trigger};

mod battle;
mod catalog;
mod control_flow;
mod presentation;
mod support;
mod world;
