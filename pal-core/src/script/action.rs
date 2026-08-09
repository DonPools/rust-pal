//! Platform-independent world mutations yielded by trigger scripts.

use crate::role::Direction;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScriptAction {
    AddItem {
        item_id: u16,
        amount: i16,
    },
    RemoveItem {
        item_id: u16,
        amount: u16,
        insufficient_entry: u16,
    },
    AdjustCash {
        amount: i16,
        insufficient_entry: u16,
    },
    PlayMusic {
        music_id: u16,
        looped: bool,
        fade_seconds: u8,
    },
    PlaySound {
        sound_id: u16,
    },
    SetBattleMusic {
        music_id: u16,
    },
    SetBattlefield {
        battlefield_id: u16,
    },
    MoveObject {
        object_id: u16,
        direction: Direction,
    },
    ChaseObject {
        object_id: u16,
        speed: u16,
        range: u16,
        floating: bool,
    },
    WalkObjectTo {
        object_id: u16,
        tile_x: u16,
        tile_y: u16,
        half: u16,
        speed: u8,
        repeat_entry: u16,
    },
    SetObjectPose {
        object_id: u16,
        direction: Option<Direction>,
        frame: Option<u16>,
    },
    SetObjectPosition {
        object_id: u16,
        x: i32,
        y: i32,
    },
    SetObjectPositionRelativeToPlayer {
        object_id: u16,
        dx: i32,
        dy: i32,
    },
    PlaceObjectInFront {
        object_id: u16,
        state: i16,
        blocked_entry: u16,
    },
    OffsetObject {
        object_id: u16,
        dx: i32,
        dy: i32,
    },
    MoveObjectBy {
        object_id: u16,
        dx: i32,
        dy: i32,
    },
    SetObjectLayer {
        object_id: u16,
        layer: i16,
    },
    SetObjectState {
        object_id: u16,
        state: i16,
    },
    SetObjectVanishTime {
        object_id: u16,
        vanish_time: i16,
    },
    HideObjectTemporarily {
        object_id: u16,
        vanish_time: i16,
    },
    SyncObjectState {
        object_id: u16,
        source_object_id: u16,
        state: i16,
    },
    AnimateObject {
        object_id: u16,
    },
    SetObjectTriggerScript {
        object_id: u16,
        script_entry: u16,
    },
    SetObjectAutoScript {
        object_id: u16,
        script_entry: u16,
    },
    SetObjectTriggerMode {
        object_id: u16,
        trigger_mode: u16,
    },
    SetObjectStates {
        first_object_id: u16,
        last_object_id: u16,
        state: i16,
    },
    SetPlayerPose {
        direction: Direction,
        frame: u8,
        party_index: u16,
    },
    SetPlayerSprite {
        role_id: u16,
        sprite_index: usize,
        reload: bool,
    },
    CheckObjectZone {
        object_id: u16,
        target_id: u16,
        range: u16,
        failure_entry: u16,
    },
    AdjustPlayerHealth {
        role_id: u16,
        hp: i16,
        mp: i16,
        apply_to_all: bool,
    },
    RevivePlayer {
        role_id: u16,
        hp_tenths: u16,
        apply_to_all: bool,
    },
    DamageEnemy {
        enemy_index: u16,
        amount: u16,
        apply_to_all: bool,
    },
    PoisonEnemy {
        enemy_index: u16,
        poison_id: u16,
        apply_to_all: bool,
    },
    PoisonPlayer {
        role_id: u16,
        poison_id: u16,
        apply_to_all: bool,
    },
    CureEnemyPoison {
        enemy_index: u16,
        poison_id: u16,
        apply_to_all: bool,
    },
    CurePlayerPoison {
        role_id: u16,
        poison_id: u16,
        apply_to_all: bool,
    },
    CurePlayerPoisonByLevel {
        role_id: u16,
        maximum_level: u16,
        apply_to_all: bool,
    },
    SetPlayerStatus {
        role_id: u16,
        status: u16,
        rounds: u16,
    },
    SetEnemyStatus {
        enemy_index: u16,
        status: u16,
        rounds: u16,
        resisted_entry: u16,
    },
    RemovePlayerStatus {
        role_id: u16,
        status: u16,
    },
    AdjustTemporaryPlayerStat {
        role_id: u16,
        attribute: u16,
        percent: i16,
    },
    SetTemporaryBattleSprite {
        role_id: u16,
        sprite: u16,
    },
    CollectEnemy {
        enemy_index: u16,
        failure_entry: u16,
    },
    TransmuteCollectedEnemies,
    HideBattleActor {
        rounds: u16,
    },
    StealEnemy {
        enemy_index: u16,
        rate: u16,
    },
    SetBattleBlow {
        amount: i16,
    },
    PlayerMagicAnimation {
        /// Zero-based battle-party index; `None` only runs the party color shift.
        player: Option<u16>,
    },
    EnableAutoBattle,
    DrainEnemyHp {
        enemy_index: u16,
        amount: u16,
    },
    FleeBattle {
        failure_entry: u16,
    },
    HalvePlayerHp {
        role_id: u16,
    },
    HalveEnemyHp {
        enemy_index: u16,
        maximum_damage: u16,
    },
    KillPlayer {
        role_id: u16,
    },
    KillEnemy {
        enemy_index: u16,
    },
    SetEnemyMagic {
        enemy_index: u16,
        magic_object: u16,
        rate: u16,
    },
    DivideEnemy {
        enemy_index: u16,
        copies: u16,
        failure_entry: u16,
    },
    SummonEnemy {
        enemy_index: u16,
        object_id: u16,
        count: u16,
        failure_entry: u16,
    },
    TransformEnemy {
        enemy_index: u16,
        object_id: u16,
    },
    EnemyEscape,
    SetBattleResult {
        result: u16,
    },
    SimulatePlayerMagic {
        enemy_index: u16,
        magic_object: u16,
        base_strength: u16,
    },
    ThrowWeapon {
        enemy_index: u16,
        magic_object: u16,
        multiplier: u16,
    },
    ScaleMagicByMp {
        role_id: u16,
        magic_object: u16,
        multiplier: u16,
    },
    ScaleMagicByCash {
        magic_object: u16,
    },
    SetEnemyChase {
        range: u16,
        cycles: u16,
    },
    LevelUpPlayer {
        role_id: u16,
        levels: u16,
    },
    HalveCash,
    SetObjectScript {
        object_id: u16,
        script_entry: u16,
        field: u16,
    },
    SetEquipmentEffect {
        role_id: u16,
        attribute: u16,
        slot: u16,
        value: i16,
    },
    EquipItem {
        role_id: u16,
        slot: u16,
        item_id: u16,
    },
    ChangePlayerAttribute {
        role_id: u16,
        attribute: u16,
        value: i16,
        absolute: bool,
    },
    RemoveEquipment {
        role_id: u16,
        slot: Option<u16>,
    },
    ChangeMagic {
        role_id: u16,
        magic_id: u16,
        add: bool,
    },
    OffsetPlayer {
        dx: i32,
        dy: i32,
        layer: u16,
    },
    SetPlayerPosition {
        tile_x: u16,
        tile_y: u16,
        half: u16,
    },
    WalkPlayerTo {
        tile_x: u16,
        tile_y: u16,
        half: u16,
        speed: u8,
        repeat_entry: u16,
    },
    RideObjectTo {
        object_id: u16,
        tile_x: u16,
        tile_y: u16,
        half: u16,
        speed: u8,
        repeat_entry: u16,
    },
    MoveViewport {
        x: i16,
        y: i16,
        frames: i16,
    },
    CollapseParty,
    ChangeScene {
        scene_number: u16,
    },
    SetSceneScripts {
        scene_number: u16,
        enter_script: Option<u16>,
        teleport_script: Option<u16>,
    },
    SetSceneMap {
        /// `None` selects the current scene (`0xffff` in the original script).
        scene_number: Option<u16>,
        map_number: u16,
    },
    SetParty {
        /// Zero-based role IDs. Empty script slots are omitted.
        members: [Option<u16>; 3],
    },
    SetPartyFollowers {
        followers: [Option<u16>; 2],
    },
}
