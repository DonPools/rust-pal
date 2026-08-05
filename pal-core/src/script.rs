//! Deterministic trigger-script execution and dialog yields.

use std::collections::BTreeMap;

use pal_assets::script::ScriptTable;

use crate::battle::{BattleRequest, BattleResult};
use crate::role::Direction;
use crate::scene::TriggerRequest;

const MAX_INSTRUCTIONS_PER_ADVANCE: usize = 1024;

/// Runtime coverage for an original PAL script opcode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpcodeSupport {
    /// The original behavior needed by the current runtime is implemented.
    Implemented,
    /// The opcode is accepted, but its visible or stateful effect is still missing.
    Stub,
    /// The opcode is known from the original engine but is rejected by this runtime.
    Unsupported,
}

macro_rules! define_script_opcodes {
    ($(
        $(#[$meta:meta])*
        $variant:ident = $value:literal, $mnemonic:literal, $description:literal, $support:ident;
    )+) => {
        /// Complete set of opcodes handled by the original PAL script engine.
        ///
        /// Values absent from this enum (`0x0032`, `0x0048`, `0x0072`, and
        /// `0x009D`) are holes in the original instruction set. `0xFFFF` is the
        /// message pseudo-instruction. Use [`ScriptOpcode::support`] to distinguish
        /// implemented instructions from compatibility stubs and unsupported ones.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        #[repr(u16)]
        pub enum ScriptOpcode {
            $(
                $(#[$meta])*
                #[doc = $description]
                $variant = $value,
            )+
        }

        impl ScriptOpcode {
            /// Every opcode present in the original interpreter, ordered by value.
            pub const ALL: &'static [Self] = &[$(Self::$variant,)+];

            pub const fn raw(self) -> u16 {
                self as u16
            }

            /// Stable assembly-style mnemonic used by diagnostics and tools.
            pub const fn mnemonic(self) -> &'static str {
                match self {
                    $(Self::$variant => $mnemonic,)+
                }
            }

            /// Short explanation of the original instruction behavior.
            pub const fn description(self) -> &'static str {
                match self {
                    $(Self::$variant => $description,)+
                }
            }

            pub const fn support(self) -> OpcodeSupport {
                match self {
                    $(Self::$variant => OpcodeSupport::$support,)+
                }
            }

            pub const fn is_implemented(self) -> bool {
                matches!(self.support(), OpcodeSupport::Implemented)
            }

            pub const fn from_raw(value: u16) -> Option<Self> {
                match value {
                    $($value => Some(Self::$variant),)+
                    _ => None,
                }
            }
        }

        impl TryFrom<u16> for ScriptOpcode {
            type Error = u16;

            fn try_from(value: u16) -> Result<Self, Self::Error> {
                Self::from_raw(value).ok_or(value)
            }
        }

        impl From<ScriptOpcode> for u16 {
            fn from(opcode: ScriptOpcode) -> Self {
                opcode.raw()
            }
        }

        impl std::fmt::Display for ScriptOpcode {
            fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str(self.mnemonic())
            }
        }
    };
}

define_script_opcodes! {
    Stop = 0x0000, "STOP", "Stop the current script.", Implemented;
    StopAndAdvance = 0x0001, "STOP_NEXT", "Stop and persist the following instruction as the next entry.", Implemented;
    StopAndReplace = 0x0002, "STOP_SET", "Stop and replace the persistent entry with operand 0.", Implemented;
    Jump = 0x0003, "JMP", "Jump unconditionally to operand 0.", Implemented;
    Call = 0x0004, "CALL", "Call the script at operand 0, optionally with another owner.", Implemented;
    Redraw = 0x0005, "REDRAW", "Redraw the screen and apply the requested delay.", Implemented;
    JumpByChance = 0x0006, "JMP_CHANCE", "Jump according to the probability in operand 0.", Implemented;
    StartBattle = 0x0007, "BATTLE", "Start a battle and branch according to its result.", Implemented;
    AdvanceEntry = 0x0008, "SET_NEXT", "Persist the following instruction as the script entry and continue.", Implemented;
    WaitFrames = 0x0009, "WAIT", "Wait for operand 0 scene frames.", Implemented;
    Confirm = 0x000A, "CONFIRM", "Ask for confirmation and jump to operand 0 when the answer is no.", Implemented;
    WalkObjectSouth = 0x000B, "WALK_S", "Walk the current event object one step south.", Implemented;
    WalkObjectWest = 0x000C, "WALK_W", "Walk the current event object one step west.", Implemented;
    WalkObjectNorth = 0x000D, "WALK_N", "Walk the current event object one step north.", Implemented;
    WalkObjectEast = 0x000E, "WALK_E", "Walk the current event object one step east.", Implemented;
    SetObjectPose = 0x000F, "OBJ_POSE", "Set the current event object's direction and/or animation frame.", Implemented;
    WalkObjectTo = 0x0010, "OBJ_WALK_TO", "Walk the current event object to a tile at normal script speed.", Implemented;
    WalkObjectToSlow = 0x0011, "OBJ_WALK_SLOW", "Walk the current event object to a tile at reduced speed.", Implemented;
    SetObjectPositionRelative = 0x0012, "OBJ_POS_REL", "Position an event object relative to the party.", Implemented;
    SetObjectPosition = 0x0013, "OBJ_POS", "Set an event object's absolute scene position.", Implemented;
    SetObjectGesture = 0x0014, "OBJ_GESTURE", "Set the current event object's gesture while facing south.", Implemented;
    SetPartyMemberPose = 0x0015, "PARTY_POSE", "Set a party member's direction and gesture.", Implemented;
    SetSelectedObjectPose = 0x0016, "OBJ_POSE_AT", "Set a selected event object's direction and gesture.", Implemented;
    SetEquipmentEffect = 0x0017, "EQUIP_EFFECT", "Set an equipment-derived extra player attribute.", Implemented;
    EquipItem = 0x0018, "EQUIP", "Equip the selected item on the script owner role.", Implemented;
    AdjustPlayerAttribute = 0x0019, "STAT_ADD", "Increase or decrease a player attribute.", Implemented;
    SetPlayerAttribute = 0x001A, "STAT_SET", "Set a player attribute to an absolute value.", Implemented;
    AdjustPlayerHp = 0x001B, "HP_ADD", "Increase or decrease one player's or the party's HP.", Implemented;
    AdjustPlayerMp = 0x001C, "MP_ADD", "Increase or decrease one player's or the party's MP.", Implemented;
    AdjustPlayerHpMp = 0x001D, "HPMP_ADD", "Increase or decrease HP and MP by the same amount.", Implemented;
    AdjustCash = 0x001E, "CASH_ADD", "Increase or decrease party cash, with an insufficient-funds branch.", Implemented;
    AddItem = 0x001F, "ITEM_ADD", "Add an item to inventory.", Implemented;
    RemoveItem = 0x0020, "ITEM_REMOVE", "Remove an item from inventory or equipped party items.", Implemented;
    DamageEnemy = 0x0021, "ENEMY_DAMAGE", "Inflict direct damage on one enemy or all enemies.", Implemented;
    RevivePlayer = 0x0022, "REVIVE", "Revive one player or all fallen party members.", Implemented;
    RemoveEquipment = 0x0023, "UNEQUIP", "Remove one or all equipment slots from a player.", Implemented;
    SetObjectAutoScript = 0x0024, "OBJ_AUTO", "Set an event object's automatic script entry.", Implemented;
    SetObjectTriggerScript = 0x0025, "OBJ_TRIGGER", "Set an event object's trigger script entry.", Implemented;
    OpenBuyMenu = 0x0026, "SHOP_BUY", "Open the specified store's buy menu.", Implemented;
    OpenSellMenu = 0x0027, "SHOP_SELL", "Open the inventory sell menu.", Implemented;
    PoisonEnemy = 0x0028, "POISON_ENEMY", "Apply a poison object to an enemy.", Implemented;
    PoisonPlayer = 0x0029, "POISON_PLAYER", "Apply a poison object to a player.", Implemented;
    CureEnemyPoison = 0x002A, "CURE_ENEMY", "Remove a specific poison object from an enemy.", Implemented;
    CurePlayerPoison = 0x002B, "CURE_PLAYER", "Remove a specific poison object from a player.", Implemented;
    CurePoisonByLevel = 0x002C, "CURE_LEVEL", "Remove player poisons up to the specified level.", Implemented;
    SetPlayerStatus = 0x002D, "STATUS_PLAYER", "Apply a temporary status to a player.", Implemented;
    SetEnemyStatus = 0x002E, "STATUS_ENEMY", "Apply a temporary status to an enemy.", Implemented;
    RemovePlayerStatus = 0x002F, "STATUS_CLEAR", "Remove a temporary status from a player.", Implemented;
    AdjustTemporaryPlayerStat = 0x0030, "STAT_TEMP", "Temporarily replace a player's extra stat effect from a percentage of the base value.", Implemented;
    SetTemporaryBattleSprite = 0x0031, "SPRITE_TEMP", "Temporarily change a player's battle sprite.", Implemented;
    CollectEnemy = 0x0033, "COLLECT_ENEMY", "Collect an enemy for later conversion into items.", Implemented;
    TransmuteCollectedEnemies = 0x0034, "COLLECT_ITEM", "Convert collected enemies into items.", Implemented;
    ShakeScreen = 0x0035, "SHAKE", "Shake the screen for the requested duration and level.", Implemented;
    SelectRngAnimation = 0x0036, "RNG_SELECT", "Select the current RNG animation resource.", Implemented;
    PlayRngAnimation = 0x0037, "RNG_PLAY", "Play frames from the selected RNG animation.", Implemented;
    TeleportParty = 0x0038, "TELEPORT", "Run the current scene's teleport script or branch on failure.", Implemented;
    DrainEnemyHp = 0x0039, "DRAIN_HP", "Drain HP from an enemy into the acting player.", Implemented;
    FleeBattle = 0x003A, "FLEE", "Attempt to flee from battle.", Implemented;
    DialogCenter = 0x003B, "DIALOG_CENTER", "Place following dialog in the middle of the screen.", Implemented;
    DialogUpper = 0x003C, "DIALOG_UPPER", "Place following dialog in the upper part of the screen.", Implemented;
    DialogLower = 0x003D, "DIALOG_LOWER", "Place following dialog in the lower part of the screen.", Implemented;
    DialogCenterWindow = 0x003E, "DIALOG_WINDOW", "Show following text in a centered window.", Implemented;
    RideObjectSlow = 0x003F, "RIDE_SLOW", "Ride the current event object to a tile at low speed.", Implemented;
    SetObjectTriggerMode = 0x0040, "OBJ_TRIGGER_MODE", "Set an event object's interaction trigger mode.", Implemented;
    MarkScriptFailed = 0x0041, "FAIL", "Mark the current script execution as failed.", Implemented;
    SimulatePlayerMagic = 0x0042, "MAGIC_SIM", "Simulate a player's magic attack in battle.", Implemented;
    PlayMusic = 0x0043, "MUSIC", "Play or stop scene background music.", Implemented;
    RideObject = 0x0044, "RIDE", "Ride the current event object to a tile at normal speed.", Implemented;
    SetBattleMusic = 0x0045, "BATTLE_MUSIC", "Set the music number for the next battle.", Implemented;
    SetPartyPosition = 0x0046, "PARTY_POS", "Set the party position on the current map.", Implemented;
    PlaySound = 0x0047, "SOUND", "Play a sound effect.", Implemented;
    SetObjectState = 0x0049, "OBJ_STATE", "Set an event object's state.", Implemented;
    SetBattlefield = 0x004A, "BATTLEFIELD", "Set the battlefield number for the next battle.", Implemented;
    HideObjectShort = 0x004B, "OBJ_HIDE_SHORT", "Hide the current event object for a short period.", Implemented;
    ChasePlayer = 0x004C, "OBJ_CHASE", "Make the current event object chase the player.", Implemented;
    WaitForKey = 0x004D, "WAIT_KEY", "Wait until the player presses a key.", Implemented;
    LoadLastSave = 0x004E, "LOAD_LAST", "Load the most recent saved game.", Implemented;
    FadeToRed = 0x004F, "FADE_RED", "Fade the screen to red for game over.", Implemented;
    FadeOut = 0x0050, "FADE_OUT", "Fade the screen out.", Implemented;
    FadeIn = 0x0051, "FADE_IN", "Fade the screen in.", Implemented;
    HideObject = 0x0052, "OBJ_HIDE", "Hide the current event object for a configurable period.", Implemented;
    UseDayPalette = 0x0053, "PALETTE_DAY", "Switch to the day palette.", Implemented;
    UseNightPalette = 0x0054, "PALETTE_NIGHT", "Switch to the night palette.", Implemented;
    AddMagic = 0x0055, "MAGIC_ADD", "Teach a magic object to a player.", Implemented;
    RemoveMagic = 0x0056, "MAGIC_REMOVE", "Remove a magic object from a player.", Implemented;
    ScaleMagicByMp = 0x0057, "MAGIC_SCALE_MP", "Set magic base damage from the consumed MP amount.", Implemented;
    JumpIfItemCountLess = 0x0058, "JLT_ITEM", "Jump when fewer than the requested number of items are held.", Implemented;
    ChangeScene = 0x0059, "SCENE", "Change to the specified scene.", Implemented;
    HalvePlayerHp = 0x005A, "HP_HALF", "Halve a player's HP.", Implemented;
    HalveEnemyHp = 0x005B, "ENEMY_HP_HALF", "Halve an enemy's HP.", Implemented;
    HideBattleActor = 0x005C, "BATTLE_HIDE", "Hide a battle actor for a period.", Implemented;
    JumpIfPlayerLacksPoison = 0x005D, "JNO_POISON", "Jump when a player lacks a specific poison.", Implemented;
    JumpIfEnemyLacksPoison = 0x005E, "JNO_ENEMY_POISON", "Jump when an enemy lacks a specific poison.", Implemented;
    KillPlayer = 0x005F, "KILL_PLAYER", "Immediately knock out a player.", Implemented;
    KillEnemy = 0x0060, "KILL_ENEMY", "Immediately knock out an enemy.", Implemented;
    JumpIfPlayerNotPoisoned = 0x0061, "JNOT_POISONED", "Jump when a player has no poison.", Implemented;
    PauseEnemyChase = 0x0062, "CHASE_PAUSE", "Pause enemy chasing for a period.", Implemented;
    SpeedUpEnemyChase = 0x0063, "CHASE_FAST", "Speed up enemy chasing for a period.", Implemented;
    JumpIfEnemyHpAbove = 0x0064, "JGT_ENEMY_HP", "Jump when enemy HP exceeds a percentage threshold.", Implemented;
    SetPlayerSprite = 0x0065, "PLAYER_SPRITE", "Set a player's scene sprite and optionally reload active party sprites.", Implemented;
    ThrowWeapon = 0x0066, "THROW_WEAPON", "Simulate a weapon-throw magic attack against an enemy.", Implemented;
    EnemyCastMagic = 0x0067, "ENEMY_MAGIC", "Set the magic and casting rate used by an enemy.", Implemented;
    JumpIfEnemyTurn = 0x0068, "JENEMY_TURN", "Jump when it is currently an enemy's turn.", Implemented;
    EnemyEscape = 0x0069, "ENEMY_FLEE", "Make the enemy party escape and terminate the battle.", Implemented;
    StealEnemy = 0x006A, "STEAL", "Steal from an enemy.", Implemented;
    BlowEnemiesAway = 0x006B, "BLOW_ENEMIES", "Apply a battlefield displacement to enemies.", Unsupported;
    OffsetObjectAndAnimate = 0x006C, "OBJ_STEP", "Offset an event object and advance its animation.", Implemented;
    SetSceneScripts = 0x006D, "SCENE_SCRIPTS", "Set a scene's enter and teleport script entries.", Implemented;
    OffsetParty = 0x006E, "PARTY_STEP", "Move the party by a signed pixel offset.", Implemented;
    SyncObjectState = 0x006F, "OBJ_STATE_SYNC", "Copy a matching state from another event object.", Implemented;
    WalkParty = 0x0070, "PARTY_WALK", "Walk the party to a tile at normal script speed.", Implemented;
    SetScreenWave = 0x0071, "SCREEN_WAVE", "Configure the screen wave effect.", Implemented;
    FadeScene = 0x0073, "FADE_SCENE", "Fade from the backed-up screen to the current scene.", Implemented;
    JumpIfPartyNotFullHp = 0x0074, "JNOT_FULL_HP", "Jump when any party member is below maximum HP.", Implemented;
    SetParty = 0x0075, "PARTY_SET", "Replace the active party membership.", Implemented;
    ShowFbp = 0x0076, "FBP_SHOW", "Show an FBP full-screen picture.", Implemented;
    StopMusic = 0x0077, "MUSIC_STOP", "Stop current music with an optional fade.", Implemented;
    NoOp = 0x0078, "NOP", "Original compatibility no-op with unknown historical purpose.", Implemented;
    JumpIfPartyContainsPlayer = 0x0079, "JHAS_PLAYER", "Jump when the specified player name is in the party.", Implemented;
    WalkPartyFast = 0x007A, "PARTY_WALK_FAST", "Walk the party to a tile at high speed.", Implemented;
    WalkPartyFastest = 0x007B, "PARTY_WALK_MAX", "Walk the party to a tile at the highest speed.", Implemented;
    WalkObjectHalfSpeed = 0x007C, "OBJ_WALK_HALF", "Walk the current event object to a tile every other original frame.", Implemented;
    OffsetObject = 0x007D, "OBJ_OFFSET", "Move an event object by a signed pixel offset.", Implemented;
    SetObjectLayer = 0x007E, "OBJ_LAYER", "Set an event object's drawing layer.", Implemented;
    MoveViewport = 0x007F, "VIEWPORT", "Move, lock, or restore the viewport.", Implemented;
    ToggleDayNightPalette = 0x0080, "PALETTE_TOGGLE", "Toggle between the day and night palettes.", Implemented;
    JumpIfNotFacingObject = 0x0081, "JNOT_FACING", "Jump when the player is not facing the specified event object.", Implemented;
    WalkObjectFast = 0x0082, "OBJ_WALK_FAST", "Walk the current event object to a tile at high speed.", Implemented;
    JumpIfObjectOutsideZone = 0x0083, "JOUTSIDE_ZONE", "Jump when an event object is outside another object's zone.", Implemented;
    PlaceUsedItemObject = 0x0084, "ITEM_PLACE", "Place the currently used item as an event object in the scene.", Implemented;
    Delay = 0x0085, "DELAY", "Delay for operand 0 periods of 80 milliseconds.", Implemented;
    JumpIfItemNotEquipped = 0x0086, "JNOT_EQUIPPED", "Jump when fewer than the requested item count are equipped.", Implemented;
    AnimateObject = 0x0087, "OBJ_ANIMATE", "Advance an event object's animation.", Implemented;
    ScaleMagicByCash = 0x0088, "MAGIC_SCALE_CASH", "Consume cash and derive magic base damage from it.", Implemented;
    SetBattleResult = 0x0089, "BATTLE_RESULT", "Set the current battle result.", Implemented;
    EnableAutoBattle = 0x008A, "AUTO_BATTLE", "Enable automatic commands for the next battle.", Implemented;
    SetPalette = 0x008B, "PALETTE_SET", "Change the current palette number.", Implemented;
    FadeColor = 0x008C, "COLOR_FADE", "Fade the screen from or to a palette color.", Implemented;
    LevelUpPlayer = 0x008D, "LEVEL_UP", "Increase a player's level.", Implemented;
    RestoreScreen = 0x008E, "SCREEN_RESTORE", "Restore the screen saved by a previous backup operation.", Implemented;
    HalveCash = 0x008F, "CASH_HALF", "Halve the party's cash.", Implemented;
    SetObjectScript = 0x0090, "OBJECT_SCRIPT", "Replace one script field in a global object definition.", Unsupported;
    JumpIfEnemyNotFirstKind = 0x0091, "JNOT_FIRST_ENEMY", "Jump when an enemy is not the first living instance of its kind.", Implemented;
    PlayerMagicAnimation = 0x0092, "MAGIC_ANIM", "Show a player's battle magic-casting animation.", Unsupported;
    FadeSceneWithUpdate = 0x0093, "SCENE_FADE_UPDATE", "Fade the screen while rebuilding the scene.", Implemented;
    JumpIfObjectStateEquals = 0x0094, "JEQ_OBJ_STATE", "Jump when an event object's state equals operand 1.", Implemented;
    JumpIfSceneEquals = 0x0095, "JEQ_SCENE", "Jump when the current scene equals operand 0.", Implemented;
    PlayEndingAnimation = 0x0096, "ENDING", "Play the DOS ending animation.", Unsupported;
    RideObjectFast = 0x0097, "RIDE_FAST", "Ride the current event object to a tile at high speed.", Implemented;
    SetPartyFollower = 0x0098, "FOLLOWER_SET", "Set or clear the party follower role.", Implemented;
    SetSceneMap = 0x0099, "SCENE_MAP", "Change the map number used by a scene.", Implemented;
    SetObjectStates = 0x009A, "OBJ_STATES", "Set one state across a contiguous event-object range.", Implemented;
    FadeToCurrentScene = 0x009B, "FADE_CURRENT", "Fade to the current scene using the original compatibility behavior.", Implemented;
    DivideEnemy = 0x009C, "ENEMY_DIVIDE", "Divide one enemy into additional copies.", Implemented;
    SummonEnemy = 0x009E, "ENEMY_SUMMON", "Make an enemy summon another monster.", Implemented;
    TransformEnemy = 0x009F, "ENEMY_TRANSFORM", "Transform an enemy into another object.", Implemented;
    QuitGame = 0x00A0, "QUIT", "Run the ending path and terminate the game.", Implemented;
    CollapseParty = 0x00A1, "PARTY_COLLAPSE", "Move every party member and trail point onto the leader.", Implemented;
    RandomSelect = 0x00A2, "RANDOM_NEXT", "Select one of the following operand 0 instructions randomly.", Implemented;
    PlayCdMusic = 0x00A3, "CD_MUSIC", "Play a CD track with normal music as fallback.", Implemented;
    ScrollFbp = 0x00A4, "FBP_SCROLL", "Scroll an FBP picture onto the screen.", Implemented;
    ShowFbpWithSprite = 0x00A5, "FBP_EFFECT", "Show an FBP picture with an ending sprite effect.", Implemented;
    BackupScreen = 0x00A6, "SCREEN_BACKUP", "Back up the current screen for a later transition.", Implemented;
    AutoScriptNoOp = 0x00A7, "AUTO_NOP", "Advance an automatic script without performing an effect.", Implemented;
    PrintMessage = 0xFFFF, "MESSAGE", "Display the message selected by operand 0.", Implemented;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DialogPosition {
    Center,
    Upper,
    #[default]
    Lower,
    CenterWindow,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScriptEvent {
    Message {
        message_id: u16,
        position: DialogPosition,
        font_color: u8,
        face_index: Option<u16>,
    },
    Waiting,
    Delay,
    Confirm {
        no_entry: u16,
    },
    OpenBuyMenu {
        store_number: u16,
    },
    OpenSellMenu,
    StartBattle(BattleRequest),
    Teleport {
        failure_entry: u16,
    },
    FadeScene {
        speed: u16,
    },
    Visual(ScriptVisual),
    WaitForKey,
    LoadLastSave,
    QuitGame,
    Action(ScriptAction),
    Condition(ScriptCondition),
    Completed {
        trigger: TriggerRequest,
        next_entry: u16,
        succeeded: bool,
    },
    Unsupported {
        trigger: TriggerRequest,
        entry: u16,
        opcode: u16,
    },
    InvalidEntry {
        trigger: TriggerRequest,
        entry: u16,
    },
    InstructionLimit {
        trigger: TriggerRequest,
        entry: u16,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScriptVisual {
    Shake {
        frames: u16,
        level: u16,
    },
    PlayRng {
        animation: u16,
        start_frame: u16,
        end_frame: Option<u16>,
        speed: u16,
    },
    FadeToRed,
    FadeOut {
        speed: u16,
    },
    FadeIn {
        speed: u16,
    },
    SetNightPalette {
        night: bool,
    },
    SetScreenWave {
        level: u16,
        progression: i16,
    },
    ShowFbp {
        index: u16,
        fade: u16,
    },
    ToggleDayNightPalette {
        update_scene: bool,
    },
    SetPalette {
        index: u16,
    },
    FadeColor {
        color: u8,
        from_color: bool,
        delay: u16,
    },
    RestoreScreen,
    FadeSceneWithUpdate {
        step: i16,
    },
    FadeToCurrentScene {
        speed: u16,
    },
    ScrollFbp {
        index: u16,
        speed: u16,
    },
    ShowFbpWithSprite {
        index: u16,
        sprite: Option<u16>,
        fade: u16,
    },
    BackupScreen,
}

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScriptCondition {
    ItemCountLess {
        item_id: u16,
        amount: i16,
        target_entry: u16,
    },
    ObjectStateEquals {
        object_id: u16,
        state: i16,
        target_entry: u16,
    },
    SceneEquals {
        scene_number: u16,
        target_entry: u16,
    },
    PartyContainsName {
        name_word_id: u16,
        target_entry: u16,
    },
    PlayerFacesObject {
        object_id: u16,
        range: u16,
        target_entry: u16,
    },
    PartyNotFullHp {
        target_entry: u16,
    },
    ItemNotEquipped {
        item_id: u16,
        amount: u16,
        target_entry: u16,
    },
    PlayerLacksPoison {
        role_id: u16,
        poison_id: u16,
        target_entry: u16,
    },
    EnemyLacksPoison {
        enemy_index: u16,
        poison_id: u16,
        target_entry: u16,
    },
    PlayerNotPoisoned {
        role_id: u16,
        target_entry: u16,
    },
    EnemyHpAbove {
        enemy_index: u16,
        percentage: u16,
        target_entry: u16,
    },
    EnemyNotFirstKind {
        enemy_index: u16,
        target_entry: u16,
    },
    EnemyTurn {
        target_entry: u16,
    },
}

#[derive(Debug, Clone, Copy)]
struct Execution {
    trigger: TriggerRequest,
    object_id: u16,
    entry: u16,
    next_entry: u16,
    dialog_position: DialogPosition,
    dialog_color: u8,
    dialog_face: Option<u16>,
    wait_frames: u16,
    wait_updates_auto_scripts: bool,
    viewport_frames_remaining: u16,
    succeeded: bool,
}

#[derive(Debug, Clone, Copy)]
struct CallFrame {
    object_id: u16,
    return_entry: u16,
}

/// One script instruction captured for development diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScriptInstructionDebug {
    pub object_id: u16,
    pub entry: u16,
    pub opcode: u16,
    pub operands: [u16; 3],
}

impl ScriptInstructionDebug {
    pub fn decoded_opcode(self) -> Option<ScriptOpcode> {
        ScriptOpcode::from_raw(self.opcode)
    }
}

/// Read-only execution details used by platform debug UIs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScriptDebugSnapshot {
    pub active: bool,
    pub trigger: Option<TriggerRequest>,
    pub last_instruction: Option<ScriptInstructionDebug>,
    pub next_instruction: Option<ScriptInstructionDebug>,
    pub call_depth: usize,
    pub wait_frames: u16,
}

pub struct ScriptRuntime {
    table: ScriptTable,
    execution: Option<Execution>,
    call_stack: Vec<CallFrame>,
    random_state: u32,
    trigger_idle_frames: BTreeMap<u16, u16>,
    last_trigger: Option<TriggerRequest>,
    last_instruction: Option<ScriptInstructionDebug>,
    pending_battle: Option<BattleRequest>,
    current_rng: u16,
}

impl ScriptRuntime {
    pub fn new(table: ScriptTable) -> Self {
        Self {
            table,
            execution: None,
            call_stack: Vec::new(),
            random_state: 0x4d59_5df4,
            trigger_idle_frames: BTreeMap::new(),
            last_trigger: None,
            last_instruction: None,
            pending_battle: None,
            current_rng: 0,
        }
    }

    pub fn start(&mut self, trigger: TriggerRequest) -> bool {
        if self.execution.is_some() || self.pending_battle.is_some() || trigger.script_entry == 0 {
            return false;
        }
        self.execution = Some(Execution {
            trigger,
            object_id: trigger.object_id,
            entry: trigger.script_entry,
            next_entry: trigger.script_entry,
            dialog_position: DialogPosition::Lower,
            dialog_color: 0x4f,
            dialog_face: None,
            wait_frames: 0,
            wait_updates_auto_scripts: false,
            viewport_frames_remaining: 0,
            succeeded: true,
        });
        self.call_stack.clear();
        self.last_trigger = Some(trigger);
        self.last_instruction = None;
        self.pending_battle = None;
        true
    }

    /// Enter a nested script while preserving the active caller.
    ///
    /// This is used by scene teleport scripts, which are invoked by an
    /// instruction in another trigger script and return to that caller when
    /// their `STOP` instruction is reached.
    pub fn call(&mut self, entry: u16, object_id: u16) -> bool {
        let Some(mut execution) = self.execution else {
            return false;
        };
        if entry == 0 || self.table.entry(entry).is_none() {
            return false;
        }
        self.call_stack.push(CallFrame {
            object_id: execution.object_id,
            return_entry: execution.entry,
        });
        execution.object_id = object_id;
        execution.entry = entry;
        execution.wait_frames = 0;
        execution.wait_updates_auto_scripts = false;
        execution.viewport_frames_remaining = 0;
        self.execution = Some(execution);
        true
    }

    pub fn is_active(&self) -> bool {
        self.execution.is_some()
    }

    pub fn is_waiting_for_battle(&self) -> bool {
        self.pending_battle.is_some()
    }

    pub fn debug_snapshot(&self) -> ScriptDebugSnapshot {
        let next_instruction = self.execution.and_then(|execution| {
            self.table
                .entry(execution.entry)
                .map(|entry| ScriptInstructionDebug {
                    object_id: execution.object_id,
                    entry: execution.entry,
                    opcode: entry.opcode,
                    operands: entry.operands,
                })
        });
        ScriptDebugSnapshot {
            active: self.execution.is_some(),
            trigger: self.last_trigger,
            last_instruction: self.last_instruction,
            next_instruction,
            call_depth: self.call_stack.len(),
            wait_frames: self.execution.map_or(0, |execution| execution.wait_frames),
        }
    }

    /// Redirect an active script after a world-state condition fails.
    pub fn branch_to(&mut self, entry: u16) -> bool {
        let Some(execution) = self.execution.as_mut() else {
            return false;
        };
        execution.entry = entry;
        execution.viewport_frames_remaining = 0;
        true
    }

    /// Record the success state of a world-dependent item effect.
    pub fn set_success(&mut self, succeeded: bool) -> bool {
        let Some(execution) = self.execution.as_mut() else {
            return false;
        };
        execution.succeeded = succeeded;
        true
    }

    /// Resume a script suspended by `BATTLE`, applying the original result branches.
    pub fn resolve_battle(&mut self, result: BattleResult) -> bool {
        let Some(request) = self.pending_battle.take() else {
            return false;
        };
        let Some(execution) = self.execution.as_mut() else {
            return false;
        };
        execution.entry = match result {
            BattleResult::Won | BattleResult::Terminated => execution.entry,
            BattleResult::Lost if request.lost_entry != 0 => request.lost_entry,
            BattleResult::Fled if request.flee_entry != 0 => request.flee_entry,
            BattleResult::Lost | BattleResult::Fled => execution.entry,
        };
        true
    }

    /// Execute until a message, completion, or unsupported instruction yields control.
    pub fn advance(&mut self) -> Option<ScriptEvent> {
        if self.pending_battle.is_some() {
            return None;
        }
        let mut execution = self.execution?;
        if execution.wait_frames > 0 {
            execution.wait_frames -= 1;
            self.execution = Some(execution);
            return Some(if execution.wait_updates_auto_scripts {
                ScriptEvent::Waiting
            } else {
                ScriptEvent::Delay
            });
        }
        for _ in 0..MAX_INSTRUCTIONS_PER_ADVANCE {
            let Some(entry) = self.table.entry(execution.entry).copied() else {
                self.execution = None;
                return Some(ScriptEvent::InvalidEntry {
                    trigger: execution.trigger,
                    entry: execution.entry,
                });
            };
            self.last_instruction = Some(ScriptInstructionDebug {
                object_id: execution.object_id,
                entry: execution.entry,
                opcode: entry.opcode,
                operands: entry.operands,
            });

            let Some(opcode) = ScriptOpcode::from_raw(entry.opcode) else {
                self.execution = None;
                return Some(ScriptEvent::Unsupported {
                    trigger: execution.trigger,
                    entry: execution.entry,
                    opcode: entry.opcode,
                });
            };
            use ScriptOpcode::*;
            match opcode {
                Stop => {
                    if let Some(frame) = self.call_stack.pop() {
                        execution.object_id = frame.object_id;
                        execution.entry = frame.return_entry;
                        continue;
                    }
                    self.execution = None;
                    return Some(ScriptEvent::Completed {
                        trigger: execution.trigger,
                        next_entry: execution.next_entry,
                        succeeded: execution.succeeded,
                    });
                }
                StopAndAdvance => {
                    let next_entry = execution.entry.wrapping_add(1);
                    if let Some(frame) = self.call_stack.pop() {
                        execution.object_id = frame.object_id;
                        execution.entry = frame.return_entry;
                        continue;
                    }
                    execution.next_entry = next_entry;
                    self.execution = None;
                    return Some(ScriptEvent::Completed {
                        trigger: execution.trigger,
                        next_entry: execution.next_entry,
                        succeeded: execution.succeeded,
                    });
                }
                StopAndReplace => {
                    if self.idle_branch(execution.object_id, entry.operands[1]) {
                        let next_entry = entry.operands[0];
                        if let Some(frame) = self.call_stack.pop() {
                            execution.object_id = frame.object_id;
                            execution.entry = frame.return_entry;
                            continue;
                        }
                        execution.next_entry = next_entry;
                        self.execution = None;
                        return Some(ScriptEvent::Completed {
                            trigger: execution.trigger,
                            next_entry,
                            succeeded: execution.succeeded,
                        });
                    }
                    execution.entry = execution.entry.wrapping_add(1);
                }
                Jump => {
                    execution.entry = if self.idle_branch(execution.object_id, entry.operands[1]) {
                        entry.operands[0]
                    } else {
                        execution.entry.wrapping_add(1)
                    };
                }
                Call => {
                    self.call_stack.push(CallFrame {
                        object_id: execution.object_id,
                        return_entry: execution.entry.wrapping_add(1),
                    });
                    execution.object_id = selected_object(entry.operands[1], execution.object_id);
                    execution.entry = entry.operands[0];
                }
                Redraw => {
                    execution.entry = execution.entry.wrapping_add(1);
                    execution.wait_frames = delay_60ms_ticks(entry.operands[1]).saturating_sub(1);
                    execution.wait_updates_auto_scripts = false;
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Delay);
                }
                StartBattle => {
                    let request = BattleRequest {
                        enemy_team: entry.operands[0],
                        lost_entry: entry.operands[1],
                        flee_entry: entry.operands[2],
                        is_boss: entry.operands[2] == 0,
                    };
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    self.pending_battle = Some(request);
                    return Some(ScriptEvent::StartBattle(request));
                }
                SetBattleMusic => {
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Action(ScriptAction::SetBattleMusic {
                        music_id: entry.operands[0],
                    }));
                }
                SetBattlefield => {
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Action(ScriptAction::SetBattlefield {
                        battlefield_id: entry.operands[0],
                    }));
                }
                ShakeScreen => {
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Visual(ScriptVisual::Shake {
                        frames: entry.operands[0],
                        level: if entry.operands[1] == 0 {
                            4
                        } else {
                            entry.operands[1]
                        },
                    }));
                }
                SelectRngAnimation => {
                    self.current_rng = entry.operands[0];
                    execution.entry = execution.entry.wrapping_add(1);
                }
                PlayRngAnimation => {
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Visual(ScriptVisual::PlayRng {
                        animation: self.current_rng,
                        start_frame: entry.operands[0],
                        end_frame: (entry.operands[1] != 0).then_some(entry.operands[1]),
                        speed: if entry.operands[2] == 0 {
                            16
                        } else {
                            entry.operands[2]
                        },
                    }));
                }
                WaitForKey => {
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::WaitForKey);
                }
                LoadLastSave => {
                    self.execution = None;
                    self.call_stack.clear();
                    return Some(ScriptEvent::LoadLastSave);
                }
                FadeToRed => {
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Visual(ScriptVisual::FadeToRed));
                }
                FadeOut => {
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Visual(ScriptVisual::FadeOut {
                        speed: entry.operands[0].max(1),
                    }));
                }
                FadeIn => {
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Visual(ScriptVisual::FadeIn {
                        speed: (entry.operands[0] as i16).max(1) as u16,
                    }));
                }
                UseDayPalette | UseNightPalette => {
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Visual(ScriptVisual::SetNightPalette {
                        night: opcode == UseNightPalette,
                    }));
                }
                RestoreScreen => {
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Visual(ScriptVisual::RestoreScreen));
                }
                RideObjectSlow | RideObject | RideObjectFast => {
                    let repeat_entry = execution.entry;
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Action(ScriptAction::RideObjectTo {
                        object_id: execution.object_id,
                        tile_x: entry.operands[0],
                        tile_y: entry.operands[1],
                        half: entry.operands[2],
                        speed: match opcode {
                            RideObjectSlow => 2,
                            RideObject => 4,
                            _ => 8,
                        },
                        repeat_entry,
                    }));
                }
                AdvanceEntry => {
                    execution.entry = execution.entry.wrapping_add(1);
                    execution.next_entry = execution.entry;
                }
                WaitFrames => {
                    execution.entry = execution.entry.wrapping_add(1);
                    execution.wait_frames = entry.operands[0].max(1) - 1;
                    execution.wait_updates_auto_scripts = true;
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Waiting);
                }
                Confirm => {
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Confirm {
                        no_entry: entry.operands[0],
                    });
                }
                JumpByChance => {
                    let roll = self.next_random_percent();
                    execution.entry = if roll >= entry.operands[0] {
                        entry.operands[1]
                    } else {
                        execution.entry.wrapping_add(1)
                    };
                }
                WalkObjectSouth | WalkObjectWest | WalkObjectNorth | WalkObjectEast => {
                    let direction = Direction::from_pal(opcode.raw() - WalkObjectSouth.raw())
                        .expect("walk opcodes always encode a valid direction");
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Action(ScriptAction::MoveObject {
                        object_id: execution.object_id,
                        direction,
                    }));
                }
                SetObjectPose => {
                    let direction = optional_direction(entry.operands[0]);
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Action(ScriptAction::SetObjectPose {
                        object_id: execution.object_id,
                        direction,
                        frame: (entry.operands[1] != 0xffff).then_some(entry.operands[1]),
                    }));
                }
                SetObjectPosition => {
                    let object_id = selected_object(entry.operands[0], execution.object_id);
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Action(ScriptAction::SetObjectPosition {
                        object_id,
                        x: i32::from(entry.operands[1]),
                        y: i32::from(entry.operands[2]),
                    }));
                }
                WalkObjectTo | WalkObjectToSlow => {
                    let repeat_entry = execution.entry;
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Action(ScriptAction::WalkObjectTo {
                        object_id: execution.object_id,
                        tile_x: entry.operands[0],
                        tile_y: entry.operands[1],
                        half: entry.operands[2],
                        speed: if opcode == WalkObjectTo { 3 } else { 2 },
                        repeat_entry,
                    }));
                }
                SetObjectPositionRelative => {
                    let object_id = selected_object(entry.operands[0], execution.object_id);
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Action(
                        ScriptAction::SetObjectPositionRelativeToPlayer {
                            object_id,
                            dx: i32::from(entry.operands[1] as i16),
                            dy: i32::from(entry.operands[2] as i16),
                        },
                    ));
                }
                SetObjectGesture => {
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Action(ScriptAction::SetObjectPose {
                        object_id: execution.object_id,
                        direction: Some(Direction::South),
                        frame: Some(entry.operands[0]),
                    }));
                }
                SetPartyMemberPose => {
                    let Some(direction) = Direction::from_pal(entry.operands[0]) else {
                        self.execution = None;
                        return Some(ScriptEvent::Unsupported {
                            trigger: execution.trigger,
                            entry: execution.entry,
                            opcode: entry.opcode,
                        });
                    };
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Action(ScriptAction::SetPlayerPose {
                        direction,
                        frame: u8::try_from(entry.operands[1]).unwrap_or(u8::MAX),
                        party_index: entry.operands[2],
                    }));
                }
                SetSelectedObjectPose if entry.operands[0] != 0 => {
                    let object_id = selected_object(entry.operands[0], execution.object_id);
                    let Some(direction) = Direction::from_pal(entry.operands[1]) else {
                        self.execution = None;
                        return Some(ScriptEvent::Unsupported {
                            trigger: execution.trigger,
                            entry: execution.entry,
                            opcode: entry.opcode,
                        });
                    };
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Action(ScriptAction::SetObjectPose {
                        object_id,
                        direction: Some(direction),
                        frame: Some(entry.operands[2]),
                    }));
                }
                SetSelectedObjectPose => execution.entry = execution.entry.wrapping_add(1),
                SetEquipmentEffect if entry.operands[0] >= 0x0b => {
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Action(ScriptAction::SetEquipmentEffect {
                        role_id: execution.object_id,
                        attribute: entry.operands[1],
                        slot: entry.operands[0] - 0x0b,
                        value: entry.operands[2] as i16,
                    }));
                }
                EquipItem if entry.operands[0] >= 0x0b => {
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Action(ScriptAction::EquipItem {
                        role_id: execution.object_id,
                        slot: entry.operands[0] - 0x0b,
                        item_id: entry.operands[1],
                    }));
                }
                AdjustPlayerAttribute | SetPlayerAttribute => {
                    let role_id = entry.operands[2]
                        .checked_sub(1)
                        .unwrap_or(execution.object_id);
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Action(ScriptAction::ChangePlayerAttribute {
                        role_id,
                        attribute: entry.operands[0],
                        value: entry.operands[1] as i16,
                        absolute: opcode == SetPlayerAttribute,
                    }));
                }
                AdjustPlayerHp | AdjustPlayerMp | AdjustPlayerHpMp => {
                    let (hp, mp) = match opcode {
                        AdjustPlayerHp => (entry.operands[1] as i16, 0),
                        AdjustPlayerMp => (0, entry.operands[1] as i16),
                        _ => (entry.operands[1] as i16, entry.operands[1] as i16),
                    };
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Action(ScriptAction::AdjustPlayerHealth {
                        role_id: execution.object_id,
                        hp,
                        mp,
                        apply_to_all: entry.operands[0] != 0,
                    }));
                }
                AdjustCash => {
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Action(ScriptAction::AdjustCash {
                        amount: entry.operands[0] as i16,
                        insufficient_entry: entry.operands[1],
                    }));
                }
                AddItem => {
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Action(ScriptAction::AddItem {
                        item_id: entry.operands[0],
                        amount: entry.operands[1] as i16,
                    }));
                }
                RemoveItem => {
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Action(ScriptAction::RemoveItem {
                        item_id: entry.operands[0],
                        amount: entry.operands[1].max(1),
                        insufficient_entry: entry.operands[2],
                    }));
                }
                RevivePlayer => {
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Action(ScriptAction::RevivePlayer {
                        role_id: execution.object_id,
                        hp_tenths: entry.operands[1],
                        apply_to_all: entry.operands[0] != 0,
                    }));
                }
                DamageEnemy => {
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Action(ScriptAction::DamageEnemy {
                        enemy_index: execution.object_id,
                        amount: entry.operands[1],
                        apply_to_all: entry.operands[0] != 0,
                    }));
                }
                PoisonEnemy => {
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Action(ScriptAction::PoisonEnemy {
                        enemy_index: execution.object_id,
                        poison_id: entry.operands[1],
                        apply_to_all: entry.operands[0] != 0,
                    }));
                }
                PoisonPlayer => {
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Action(ScriptAction::PoisonPlayer {
                        role_id: execution.object_id,
                        poison_id: entry.operands[1],
                        apply_to_all: entry.operands[0] != 0,
                    }));
                }
                CureEnemyPoison => {
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Action(ScriptAction::CureEnemyPoison {
                        enemy_index: execution.object_id,
                        poison_id: entry.operands[1],
                        apply_to_all: entry.operands[0] != 0,
                    }));
                }
                CurePlayerPoison => {
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Action(ScriptAction::CurePlayerPoison {
                        role_id: execution.object_id,
                        poison_id: entry.operands[1],
                        apply_to_all: entry.operands[0] != 0,
                    }));
                }
                CurePoisonByLevel => {
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Action(ScriptAction::CurePlayerPoisonByLevel {
                        role_id: execution.object_id,
                        maximum_level: entry.operands[1],
                        apply_to_all: entry.operands[0] != 0,
                    }));
                }
                SetPlayerStatus => {
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Action(ScriptAction::SetPlayerStatus {
                        role_id: execution.object_id,
                        status: entry.operands[0],
                        rounds: entry.operands[1],
                    }));
                }
                SetEnemyStatus => {
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Action(ScriptAction::SetEnemyStatus {
                        enemy_index: execution.object_id,
                        status: entry.operands[0],
                        rounds: entry.operands[1],
                        resisted_entry: entry.operands[2],
                    }));
                }
                RemovePlayerStatus => {
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Action(ScriptAction::RemovePlayerStatus {
                        role_id: execution.object_id,
                        status: entry.operands[0],
                    }));
                }
                AdjustTemporaryPlayerStat => {
                    let role_id = entry.operands[2]
                        .checked_sub(1)
                        .unwrap_or(execution.object_id);
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Action(
                        ScriptAction::AdjustTemporaryPlayerStat {
                            role_id,
                            attribute: entry.operands[0],
                            percent: entry.operands[1] as i16,
                        },
                    ));
                }
                SetTemporaryBattleSprite => {
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Action(
                        ScriptAction::SetTemporaryBattleSprite {
                            role_id: execution.object_id,
                            sprite: entry.operands[0],
                        },
                    ));
                }
                CollectEnemy => {
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Action(ScriptAction::CollectEnemy {
                        enemy_index: execution.object_id,
                        failure_entry: entry.operands[0],
                    }));
                }
                TransmuteCollectedEnemies => {
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Action(ScriptAction::TransmuteCollectedEnemies));
                }
                SimulatePlayerMagic => {
                    let selected = (entry.operands[2] as i16).wrapping_sub(1);
                    let enemy_index = if selected < 0 {
                        execution.object_id
                    } else {
                        selected as u16
                    };
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Action(ScriptAction::SimulatePlayerMagic {
                        enemy_index,
                        magic_object: entry.operands[0],
                        base_strength: entry.operands[1],
                    }));
                }
                ThrowWeapon => {
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Action(ScriptAction::ThrowWeapon {
                        enemy_index: execution.object_id,
                        magic_object: entry.operands[0],
                        multiplier: entry.operands[1],
                    }));
                }
                ScaleMagicByMp => {
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Action(ScriptAction::ScaleMagicByMp {
                        role_id: execution.object_id,
                        magic_object: entry.operands[0],
                        multiplier: if entry.operands[1] == 0 {
                            8
                        } else {
                            entry.operands[1]
                        },
                    }));
                }
                ScaleMagicByCash => {
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Action(ScriptAction::ScaleMagicByCash {
                        magic_object: entry.operands[0],
                    }));
                }
                PauseEnemyChase | SpeedUpEnemyChase => {
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Action(ScriptAction::SetEnemyChase {
                        range: if opcode == PauseEnemyChase { 0 } else { 3 },
                        cycles: entry.operands[0],
                    }));
                }
                LevelUpPlayer => {
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Action(ScriptAction::LevelUpPlayer {
                        role_id: execution.object_id,
                        levels: entry.operands[0],
                    }));
                }
                HalveCash => {
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Action(ScriptAction::HalveCash));
                }
                DrainEnemyHp => {
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Action(ScriptAction::DrainEnemyHp {
                        enemy_index: execution.object_id,
                        amount: entry.operands[0],
                    }));
                }
                FleeBattle => {
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Action(ScriptAction::FleeBattle {
                        failure_entry: entry.operands[0],
                    }));
                }
                HalvePlayerHp => {
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Action(ScriptAction::HalvePlayerHp {
                        role_id: execution.object_id,
                    }));
                }
                HalveEnemyHp => {
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Action(ScriptAction::HalveEnemyHp {
                        enemy_index: execution.object_id,
                        maximum_damage: entry.operands[0],
                    }));
                }
                HideBattleActor => {
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Action(ScriptAction::HideBattleActor {
                        rounds: entry.operands[0],
                    }));
                }
                KillPlayer => {
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Action(ScriptAction::KillPlayer {
                        role_id: execution.object_id,
                    }));
                }
                KillEnemy => {
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Action(ScriptAction::KillEnemy {
                        enemy_index: execution.object_id,
                    }));
                }
                EnemyCastMagic => {
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Action(ScriptAction::SetEnemyMagic {
                        enemy_index: execution.object_id,
                        magic_object: entry.operands[0],
                        rate: entry.operands[1],
                    }));
                }
                EnemyEscape => {
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Action(ScriptAction::EnemyEscape));
                }
                StealEnemy => {
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Action(ScriptAction::StealEnemy {
                        enemy_index: execution.object_id,
                        rate: entry.operands[0],
                    }));
                }
                SetBattleResult => {
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Action(ScriptAction::SetBattleResult {
                        result: entry.operands[0],
                    }));
                }
                EnableAutoBattle => {
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Action(ScriptAction::EnableAutoBattle));
                }
                DivideEnemy => {
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Action(ScriptAction::DivideEnemy {
                        enemy_index: execution.object_id,
                        copies: entry.operands[0],
                        failure_entry: entry.operands[1],
                    }));
                }
                SummonEnemy => {
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Action(ScriptAction::SummonEnemy {
                        enemy_index: execution.object_id,
                        object_id: entry.operands[0],
                        count: entry.operands[1],
                        failure_entry: entry.operands[2],
                    }));
                }
                TransformEnemy => {
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Action(ScriptAction::TransformEnemy {
                        enemy_index: execution.object_id,
                        object_id: entry.operands[0],
                    }));
                }
                RemoveEquipment => {
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Action(ScriptAction::RemoveEquipment {
                        role_id: entry.operands[0],
                        slot: entry.operands[1].checked_sub(1),
                    }));
                }
                OpenBuyMenu => {
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::OpenBuyMenu {
                        store_number: entry.operands[0],
                    });
                }
                OpenSellMenu => {
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::OpenSellMenu);
                }
                TeleportParty => {
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Teleport {
                        failure_entry: entry.operands[0],
                    });
                }
                SetObjectAutoScript if entry.operands[0] != 0 => {
                    let object_id = selected_object(entry.operands[0], execution.object_id);
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Action(ScriptAction::SetObjectAutoScript {
                        object_id,
                        script_entry: entry.operands[1],
                    }));
                }
                SetObjectAutoScript => execution.entry = execution.entry.wrapping_add(1),
                SetObjectTriggerScript if entry.operands[0] != 0 => {
                    let object_id = selected_object(entry.operands[0], execution.object_id);
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Action(ScriptAction::SetObjectTriggerScript {
                        object_id,
                        script_entry: entry.operands[1],
                    }));
                }
                SetObjectTriggerScript => execution.entry = execution.entry.wrapping_add(1),
                PlayMusic => {
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Action(ScriptAction::PlayMusic {
                        music_id: entry.operands[0],
                        looped: entry.operands[1] != 1,
                        fade_seconds: u8::from(entry.operands[1] == 3 && entry.operands[0] != 9)
                            * 3,
                    }));
                }
                PlaySound => {
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Action(ScriptAction::PlaySound {
                        sound_id: entry.operands[0],
                    }));
                }
                SetObjectState if entry.operands[0] != 0 => {
                    let object_id = selected_object(entry.operands[0], execution.object_id);
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Action(ScriptAction::SetObjectState {
                        object_id,
                        state: entry.operands[1] as i16,
                    }));
                }
                SetObjectState => execution.entry = execution.entry.wrapping_add(1),
                HideObjectShort => {
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Action(ScriptAction::SetObjectVanishTime {
                        object_id: execution.object_id,
                        vanish_time: -15,
                    }));
                }
                HideObject => {
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Action(ScriptAction::HideObjectTemporarily {
                        object_id: execution.object_id,
                        vanish_time: if entry.operands[0] == 0 {
                            800
                        } else {
                            entry.operands[0] as i16
                        },
                    }));
                }
                SetPartyPosition => {
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Action(ScriptAction::SetPlayerPosition {
                        tile_x: entry.operands[0],
                        tile_y: entry.operands[1],
                        half: entry.operands[2],
                    }));
                }
                JumpIfItemCountLess => {
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Condition(ScriptCondition::ItemCountLess {
                        item_id: entry.operands[0],
                        amount: entry.operands[1] as i16,
                        target_entry: entry.operands[2],
                    }));
                }
                ChangeScene => {
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Action(ScriptAction::ChangeScene {
                        scene_number: entry.operands[0],
                    }));
                }
                JumpIfPlayerLacksPoison => {
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Condition(ScriptCondition::PlayerLacksPoison {
                        role_id: execution.object_id,
                        poison_id: entry.operands[0],
                        target_entry: entry.operands[1],
                    }));
                }
                JumpIfEnemyLacksPoison => {
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Condition(ScriptCondition::EnemyLacksPoison {
                        enemy_index: execution.object_id,
                        poison_id: entry.operands[0],
                        target_entry: entry.operands[1],
                    }));
                }
                JumpIfPlayerNotPoisoned => {
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Condition(ScriptCondition::PlayerNotPoisoned {
                        role_id: execution.object_id,
                        target_entry: entry.operands[0],
                    }));
                }
                JumpIfEnemyHpAbove => {
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Condition(ScriptCondition::EnemyHpAbove {
                        enemy_index: execution.object_id,
                        percentage: entry.operands[0],
                        target_entry: entry.operands[1],
                    }));
                }
                JumpIfEnemyNotFirstKind => {
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Condition(ScriptCondition::EnemyNotFirstKind {
                        enemy_index: execution.object_id,
                        target_entry: entry.operands[0],
                    }));
                }
                JumpIfEnemyTurn => {
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Condition(ScriptCondition::EnemyTurn {
                        target_entry: entry.operands[0],
                    }));
                }
                SetPlayerSprite => {
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Action(ScriptAction::SetPlayerSprite {
                        role_id: entry.operands[0],
                        sprite_index: usize::from(entry.operands[1]),
                        reload: entry.operands[2] != 0,
                    }));
                }
                JumpIfObjectOutsideZone => {
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Action(ScriptAction::CheckObjectZone {
                        object_id: execution.object_id,
                        target_id: entry.operands[0],
                        range: entry.operands[1],
                        failure_entry: entry.operands[2],
                    }));
                }
                OffsetObjectAndAnimate => {
                    let object_id = selected_object(entry.operands[0], execution.object_id);
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Action(ScriptAction::OffsetObject {
                        object_id,
                        dx: i32::from(entry.operands[1] as i16),
                        dy: i32::from(entry.operands[2] as i16),
                    }));
                }
                SetSceneScripts if entry.operands[0] != 0 => {
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    let clear = entry.operands[1] == 0 && entry.operands[2] == 0;
                    return Some(ScriptEvent::Action(ScriptAction::SetSceneScripts {
                        scene_number: entry.operands[0],
                        enter_script: (clear || entry.operands[1] != 0)
                            .then_some(entry.operands[1]),
                        teleport_script: (clear || entry.operands[2] != 0)
                            .then_some(entry.operands[2]),
                    }));
                }
                SetSceneScripts => execution.entry = execution.entry.wrapping_add(1),
                OffsetParty => {
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Action(ScriptAction::OffsetPlayer {
                        dx: i32::from(entry.operands[0] as i16),
                        dy: i32::from(entry.operands[1] as i16),
                    }));
                }
                WalkParty => {
                    let repeat_entry = execution.entry;
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Action(ScriptAction::WalkPlayerTo {
                        tile_x: entry.operands[0],
                        tile_y: entry.operands[1],
                        half: entry.operands[2],
                        speed: 2,
                        repeat_entry,
                    }));
                }
                SetScreenWave => {
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Visual(ScriptVisual::SetScreenWave {
                        level: entry.operands[0],
                        progression: entry.operands[1] as i16,
                    }));
                }
                FadeScene => {
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::FadeScene {
                        speed: entry.operands[0],
                    });
                }
                ShowFbp => {
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Visual(ScriptVisual::ShowFbp {
                        index: entry.operands[0],
                        fade: entry.operands[1],
                    }));
                }
                SetParty => {
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    let mut members = entry.operands.map(|role| role.checked_sub(1));
                    if members.iter().all(Option::is_none) {
                        members[0] = Some(0);
                    }
                    return Some(ScriptEvent::Action(ScriptAction::SetParty { members }));
                }
                JumpIfPartyNotFullHp => {
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Condition(ScriptCondition::PartyNotFullHp {
                        target_entry: entry.operands[0],
                    }));
                }
                StopMusic => {
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Action(ScriptAction::PlayMusic {
                        music_id: 0,
                        looped: false,
                        fade_seconds: if entry.operands[0] == 0 {
                            2
                        } else {
                            u8::try_from(entry.operands[0].saturating_mul(3)).unwrap_or(u8::MAX)
                        },
                    }));
                }
                JumpIfPartyContainsPlayer => {
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Condition(ScriptCondition::PartyContainsName {
                        name_word_id: entry.operands[0],
                        target_entry: entry.operands[1],
                    }));
                }
                WalkPartyFast | WalkPartyFastest => {
                    let repeat_entry = execution.entry;
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Action(ScriptAction::WalkPlayerTo {
                        tile_x: entry.operands[0],
                        tile_y: entry.operands[1],
                        half: entry.operands[2],
                        speed: if opcode == WalkPartyFast { 4 } else { 8 },
                        repeat_entry,
                    }));
                }
                WalkObjectHalfSpeed | WalkObjectFast => {
                    let repeat_entry = execution.entry;
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Action(ScriptAction::WalkObjectTo {
                        object_id: execution.object_id,
                        tile_x: entry.operands[0],
                        tile_y: entry.operands[1],
                        half: entry.operands[2],
                        // 0x007c moves four pixels every other original frame.
                        speed: if opcode == WalkObjectHalfSpeed { 2 } else { 8 },
                        repeat_entry,
                    }));
                }
                OffsetObject => {
                    let object_id = selected_object(entry.operands[0], execution.object_id);
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Action(ScriptAction::MoveObjectBy {
                        object_id,
                        dx: i32::from(entry.operands[1] as i16),
                        dy: i32::from(entry.operands[2] as i16),
                    }));
                }
                SetObjectLayer => {
                    let object_id = selected_object(entry.operands[0], execution.object_id);
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Action(ScriptAction::SetObjectLayer {
                        object_id,
                        layer: entry.operands[1] as i16,
                    }));
                }
                MoveViewport => {
                    let frames = entry.operands[2] as i16;
                    if (entry.operands[0] == 0 && entry.operands[1] == 0) || frames == -1 {
                        execution.entry = execution.entry.wrapping_add(1);
                    } else {
                        if execution.viewport_frames_remaining == 0 {
                            execution.viewport_frames_remaining =
                                u16::try_from(frames).unwrap_or(1).max(1);
                        }
                        execution.viewport_frames_remaining -= 1;
                        if execution.viewport_frames_remaining == 0 {
                            execution.entry = execution.entry.wrapping_add(1);
                        }
                    }
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Action(ScriptAction::MoveViewport {
                        x: entry.operands[0] as i16,
                        y: entry.operands[1] as i16,
                        frames: if frames == -1 { -1 } else { 1 },
                    }));
                }
                ToggleDayNightPalette => {
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Visual(ScriptVisual::ToggleDayNightPalette {
                        update_scene: entry.operands[0] == 0,
                    }));
                }
                SetPalette => {
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Visual(ScriptVisual::SetPalette {
                        index: entry.operands[0],
                    }));
                }
                FadeColor => {
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Visual(ScriptVisual::FadeColor {
                        color: entry.operands[0] as u8,
                        from_color: entry.operands[1] != 0,
                        delay: entry.operands[2],
                    }));
                }
                FadeSceneWithUpdate => {
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Visual(ScriptVisual::FadeSceneWithUpdate {
                        step: entry.operands[0] as i16,
                    }));
                }
                NoOp => execution.entry = execution.entry.wrapping_add(1),
                JumpIfNotFacingObject => {
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Condition(ScriptCondition::PlayerFacesObject {
                        object_id: entry.operands[0],
                        range: entry.operands[1],
                        target_entry: entry.operands[2],
                    }));
                }
                PlaceUsedItemObject => {
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Action(ScriptAction::PlaceObjectInFront {
                        object_id: entry.operands[0],
                        state: entry.operands[1] as i16,
                        blocked_entry: entry.operands[2],
                    }));
                }
                JumpIfItemNotEquipped => {
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Condition(ScriptCondition::ItemNotEquipped {
                        item_id: entry.operands[0],
                        amount: entry.operands[1],
                        target_entry: entry.operands[2],
                    }));
                }
                SyncObjectState => {
                    let source_object_id = selected_object(entry.operands[0], execution.object_id);
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Action(ScriptAction::SyncObjectState {
                        object_id: execution.object_id,
                        source_object_id,
                        state: entry.operands[1] as i16,
                    }));
                }
                AnimateObject => {
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Action(ScriptAction::AnimateObject {
                        object_id: execution.object_id,
                    }));
                }
                JumpIfObjectStateEquals => {
                    let object_id = selected_object(entry.operands[0], execution.object_id);
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Condition(ScriptCondition::ObjectStateEquals {
                        object_id,
                        state: entry.operands[1] as i16,
                        target_entry: entry.operands[2],
                    }));
                }
                JumpIfSceneEquals => {
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Condition(ScriptCondition::SceneEquals {
                        scene_number: entry.operands[0],
                        target_entry: entry.operands[1],
                    }));
                }
                SetPartyFollower => {
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Action(ScriptAction::SetPartyFollowers {
                        followers: [entry.operands[0], entry.operands[1]]
                            .map(|role_id| (role_id != 0).then_some(role_id)),
                    }));
                }
                SetSceneMap if entry.operands[1] != 0 => {
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Action(ScriptAction::SetSceneMap {
                        scene_number: (entry.operands[0] != 0xffff).then_some(entry.operands[0]),
                        map_number: entry.operands[1],
                    }));
                }
                SetObjectStates
                    if entry.operands[0] != 0 && entry.operands[0] <= entry.operands[1] =>
                {
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Action(ScriptAction::SetObjectStates {
                        first_object_id: entry.operands[0],
                        last_object_id: entry.operands[1],
                        state: entry.operands[2] as i16,
                    }));
                }
                CollapseParty => {
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Action(ScriptAction::CollapseParty));
                }
                RandomSelect if entry.operands[0] != 0 => {
                    let choices = entry.operands[0];
                    let choice = self.next_random_percent().wrapping_sub(1) % choices;
                    execution.entry = execution.entry.wrapping_add(choice).wrapping_add(1);
                }
                FadeToCurrentScene => {
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Visual(ScriptVisual::FadeToCurrentScene {
                        speed: 2,
                    }));
                }
                QuitGame => {
                    self.execution = None;
                    self.call_stack.clear();
                    return Some(ScriptEvent::QuitGame);
                }
                ScrollFbp => {
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Visual(ScriptVisual::ScrollFbp {
                        index: entry.operands[0],
                        speed: entry.operands[2],
                    }));
                }
                ShowFbpWithSprite => {
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Visual(ScriptVisual::ShowFbpWithSprite {
                        index: entry.operands[0],
                        sprite: (entry.operands[1] != 0xffff).then_some(entry.operands[1]),
                        fade: entry.operands[2],
                    }));
                }
                BackupScreen => {
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Visual(ScriptVisual::BackupScreen));
                }
                PlayCdMusic => {
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Action(ScriptAction::PlayMusic {
                        music_id: entry.operands[1],
                        looped: true,
                        fade_seconds: 0,
                    }));
                }
                AutoScriptNoOp => execution.entry = execution.entry.wrapping_add(1),
                DialogCenter => {
                    execution.dialog_position = DialogPosition::Center;
                    if entry.operands[0] != 0 {
                        execution.dialog_color = entry.operands[0] as u8;
                    }
                    execution.dialog_face = None;
                    execution.entry = execution.entry.wrapping_add(1);
                }
                DialogUpper => {
                    execution.dialog_position = DialogPosition::Upper;
                    if entry.operands[1] != 0 {
                        execution.dialog_color = entry.operands[1] as u8;
                    }
                    execution.dialog_face = (entry.operands[0] != 0).then_some(entry.operands[0]);
                    execution.entry = execution.entry.wrapping_add(1);
                }
                DialogLower => {
                    execution.dialog_position = DialogPosition::Lower;
                    if entry.operands[1] != 0 {
                        execution.dialog_color = entry.operands[1] as u8;
                    }
                    execution.dialog_face = (entry.operands[0] != 0).then_some(entry.operands[0]);
                    execution.entry = execution.entry.wrapping_add(1);
                }
                DialogCenterWindow => {
                    execution.dialog_position = DialogPosition::CenterWindow;
                    if entry.operands[0] != 0 {
                        execution.dialog_color = entry.operands[0] as u8;
                    }
                    execution.dialog_face = None;
                    execution.entry = execution.entry.wrapping_add(1);
                }
                SetObjectTriggerMode if entry.operands[0] != 0 => {
                    let object_id = selected_object(entry.operands[0], execution.trigger.object_id);
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Action(ScriptAction::SetObjectTriggerMode {
                        object_id,
                        trigger_mode: entry.operands[1],
                    }));
                }
                SetObjectTriggerMode => execution.entry = execution.entry.wrapping_add(1),
                MarkScriptFailed => {
                    execution.succeeded = false;
                    execution.entry = execution.entry.wrapping_add(1);
                }
                AddMagic | RemoveMagic => {
                    let role_id = entry.operands[1]
                        .checked_sub(1)
                        .unwrap_or(execution.object_id);
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Action(ScriptAction::ChangeMagic {
                        role_id,
                        magic_id: entry.operands[0],
                        add: opcode == AddMagic,
                    }));
                }
                Delay => {
                    execution.entry = execution.entry.wrapping_add(1);
                    execution.wait_frames = delay_80ms_ticks(entry.operands[0]).saturating_sub(1);
                    execution.wait_updates_auto_scripts = false;
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Delay);
                }
                PrintMessage => {
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Message {
                        message_id: entry.operands[0],
                        position: execution.dialog_position,
                        font_color: execution.dialog_color,
                        face_index: execution.dialog_face,
                    });
                }
                // Known original instructions that the trigger runtime does not implement yet.
                SetEquipmentEffect | EquipItem | ChasePlayer | BlowEnemiesAway
                | SetObjectScript | PlayerMagicAnimation | PlayEndingAnimation => {
                    self.execution = None;
                    return Some(ScriptEvent::Unsupported {
                        trigger: execution.trigger,
                        entry: execution.entry,
                        opcode: opcode.raw(),
                    });
                }
                // Implemented instructions with malformed operands are rejected explicitly.
                SetObjectStates | SetSceneMap | RandomSelect => {
                    self.execution = None;
                    return Some(ScriptEvent::Unsupported {
                        trigger: execution.trigger,
                        entry: execution.entry,
                        opcode: opcode.raw(),
                    });
                }
            }
        }

        self.execution = None;
        self.call_stack.clear();
        Some(ScriptEvent::InstructionLimit {
            trigger: execution.trigger,
            entry: execution.entry,
        })
    }

    fn next_random_percent(&mut self) -> u16 {
        self.random_state = self
            .random_state
            .wrapping_mul(1_664_525)
            .wrapping_add(1_013_904_223);
        ((self.random_state >> 16) % 100 + 1) as u16
    }

    fn idle_branch(&mut self, object_id: u16, limit: u16) -> bool {
        if limit == 0 {
            return true;
        }
        let idle = self
            .trigger_idle_frames
            .entry(object_id)
            .or_default()
            .wrapping_add(1);
        if idle < limit {
            self.trigger_idle_frames.insert(object_id, idle);
            true
        } else {
            self.trigger_idle_frames.remove(&object_id);
            false
        }
    }
}

fn selected_object(selector: u16, current: u16) -> u16 {
    if selector == 0 || selector == 0xffff {
        current
    } else {
        selector
    }
}

fn optional_direction(value: u16) -> Option<Direction> {
    (value != 0xffff)
        .then(|| Direction::from_pal(value))
        .flatten()
}

fn delay_80ms_ticks(periods: u16) -> u16 {
    const SCRIPT_TICK_MS: u32 = 50;
    let milliseconds = u32::from(periods) * 80;
    milliseconds
        .div_ceil(SCRIPT_TICK_MS)
        .max(1)
        .min(u32::from(u16::MAX)) as u16
}

fn delay_60ms_ticks(periods: u16) -> u16 {
    const SCRIPT_TICK_MS: u32 = 50;
    let periods = u32::from(periods.max(1));
    (periods * 60)
        .div_ceil(SCRIPT_TICK_MS)
        .min(u32::from(u16::MAX)) as u16
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::TriggerKind;

    fn table(entries: &[[u16; 4]]) -> ScriptTable {
        let data = entries
            .iter()
            .flat_map(|entry| entry.iter().flat_map(|value| value.to_le_bytes()))
            .collect::<Vec<_>>();
        ScriptTable::parse(&data).unwrap()
    }

    #[test]
    fn opcode_catalog_covers_every_original_instruction_once() {
        assert_eq!(ScriptOpcode::ALL.len(), 165);
        for pair in ScriptOpcode::ALL.windows(2) {
            assert!(pair[0].raw() < pair[1].raw());
        }
        for &opcode in ScriptOpcode::ALL {
            assert_eq!(ScriptOpcode::from_raw(opcode.raw()), Some(opcode));
            assert!(!opcode.mnemonic().is_empty());
            assert!(!opcode.description().is_empty());
        }

        let support_counts = ScriptOpcode::ALL
            .iter()
            .fold([0usize; 3], |mut counts, opcode| {
                let index = match opcode.support() {
                    OpcodeSupport::Implemented => 0,
                    OpcodeSupport::Stub => 1,
                    OpcodeSupport::Unsupported => 2,
                };
                counts[index] += 1;
                counts
            });
        assert_eq!(support_counts, [161, 0, 4]);

        for hole in [0x0032, 0x0048, 0x0072, 0x009d] {
            assert_eq!(ScriptOpcode::from_raw(hole), None);
        }
        assert_eq!(
            ScriptOpcode::AdjustPlayerHp.support(),
            OpcodeSupport::Implemented
        );
        assert_eq!(
            ScriptOpcode::FadeScene.support(),
            OpcodeSupport::Implemented
        );
        assert_eq!(
            ScriptOpcode::StartBattle.support(),
            OpcodeSupport::Implemented
        );
        assert!(ScriptOpcode::PrintMessage.is_implemented());
        assert_eq!(ScriptOpcode::FadeScene.to_string(), "FADE_SCENE");
    }

    #[test]
    fn every_unsupported_opcode_has_an_explicit_trigger_match_arm() {
        for &opcode in ScriptOpcode::ALL {
            if opcode.support() != OpcodeSupport::Unsupported {
                continue;
            }
            let mut runtime = ScriptRuntime::new(table(&[[0, 0, 0, 0], [opcode.raw(), 0, 0, 0]]));
            runtime.start(trigger(1));
            assert_eq!(
                runtime.advance(),
                Some(ScriptEvent::Unsupported {
                    trigger: trigger(1),
                    entry: 1,
                    opcode: opcode.raw(),
                }),
                "{} ({:04X}) was not rejected by its explicit match arm",
                opcode.mnemonic(),
                opcode.raw()
            );
        }

        // ChasePlayer is implemented by the auto-script scheduler, but not by
        // the trigger-script interpreter.
        let mut runtime = ScriptRuntime::new(table(&[
            [0, 0, 0, 0],
            [ScriptOpcode::ChasePlayer.raw(), 0, 0, 0],
        ]));
        runtime.start(trigger(1));
        assert!(matches!(
            runtime.advance(),
            Some(ScriptEvent::Unsupported { opcode: 0x004c, .. })
        ));
    }

    #[test]
    fn visual_opcodes_yield_typed_blocking_events() {
        let mut runtime = ScriptRuntime::new(table(&[
            [0, 0, 0, 0],
            [ScriptOpcode::ShakeScreen.raw(), 3, 0, 0],
            [ScriptOpcode::SelectRngAnimation.raw(), 7, 0, 0],
            [ScriptOpcode::PlayRngAnimation.raw(), 2, 5, 0],
            [ScriptOpcode::FadeOut.raw(), 0, 0, 0],
            [ScriptOpcode::FadeIn.raw(), 0xffff, 0, 0],
            [ScriptOpcode::UseNightPalette.raw(), 0, 0, 0],
            [ScriptOpcode::SetScreenWave.raw(), 4, 0xffff, 0],
            [ScriptOpcode::ShowFbp.raw(), 9, 2, 0],
            [ScriptOpcode::ToggleDayNightPalette.raw(), 0, 0, 0],
            [ScriptOpcode::SetPalette.raw(), 3, 0, 0],
            [ScriptOpcode::FadeColor.raw(), 0x4f, 1, 2],
            [ScriptOpcode::RestoreScreen.raw(), 0, 0, 0],
            [ScriptOpcode::FadeSceneWithUpdate.raw(), 0xfffe, 0, 0],
            [ScriptOpcode::FadeToCurrentScene.raw(), 0, 0, 0],
            [ScriptOpcode::ScrollFbp.raw(), 6, 0, 4],
            [ScriptOpcode::ShowFbpWithSprite.raw(), 8, 0xffff, 3],
            [ScriptOpcode::BackupScreen.raw(), 0, 0, 0],
            [ScriptOpcode::AutoScriptNoOp.raw(), 0, 0, 0],
            [ScriptOpcode::WaitForKey.raw(), 0, 0, 0],
            [ScriptOpcode::Stop.raw(), 0, 0, 0],
        ]));
        runtime.start(trigger(1));
        assert_eq!(
            runtime.advance(),
            Some(ScriptEvent::Visual(ScriptVisual::Shake {
                frames: 3,
                level: 4
            }))
        );
        assert_eq!(
            runtime.advance(),
            Some(ScriptEvent::Visual(ScriptVisual::PlayRng {
                animation: 7,
                start_frame: 2,
                end_frame: Some(5),
                speed: 16,
            }))
        );
        assert_eq!(
            runtime.advance(),
            Some(ScriptEvent::Visual(ScriptVisual::FadeOut { speed: 1 }))
        );
        assert_eq!(
            runtime.advance(),
            Some(ScriptEvent::Visual(ScriptVisual::FadeIn { speed: 1 }))
        );
        assert_eq!(
            runtime.advance(),
            Some(ScriptEvent::Visual(ScriptVisual::SetNightPalette {
                night: true
            }))
        );
        assert_eq!(
            runtime.advance(),
            Some(ScriptEvent::Visual(ScriptVisual::SetScreenWave {
                level: 4,
                progression: -1,
            }))
        );
        assert_eq!(
            runtime.advance(),
            Some(ScriptEvent::Visual(ScriptVisual::ShowFbp {
                index: 9,
                fade: 2,
            }))
        );
        assert_eq!(
            runtime.advance(),
            Some(ScriptEvent::Visual(ScriptVisual::ToggleDayNightPalette {
                update_scene: true
            }))
        );
        assert_eq!(
            runtime.advance(),
            Some(ScriptEvent::Visual(ScriptVisual::SetPalette { index: 3 }))
        );
        assert_eq!(
            runtime.advance(),
            Some(ScriptEvent::Visual(ScriptVisual::FadeColor {
                color: 0x4f,
                from_color: true,
                delay: 2,
            }))
        );
        assert_eq!(
            runtime.advance(),
            Some(ScriptEvent::Visual(ScriptVisual::RestoreScreen))
        );
        assert_eq!(
            runtime.advance(),
            Some(ScriptEvent::Visual(ScriptVisual::FadeSceneWithUpdate {
                step: -2
            }))
        );
        assert_eq!(
            runtime.advance(),
            Some(ScriptEvent::Visual(ScriptVisual::FadeToCurrentScene {
                speed: 2
            }))
        );
        assert_eq!(
            runtime.advance(),
            Some(ScriptEvent::Visual(ScriptVisual::ScrollFbp {
                index: 6,
                speed: 4,
            }))
        );
        assert_eq!(
            runtime.advance(),
            Some(ScriptEvent::Visual(ScriptVisual::ShowFbpWithSprite {
                index: 8,
                sprite: None,
                fade: 3,
            }))
        );
        assert_eq!(
            runtime.advance(),
            Some(ScriptEvent::Visual(ScriptVisual::BackupScreen))
        );
        assert_eq!(runtime.advance(), Some(ScriptEvent::WaitForKey));
        assert!(matches!(
            runtime.advance(),
            Some(ScriptEvent::Completed { .. })
        ));
    }

    #[test]
    fn load_and_quit_events_stop_the_active_script() {
        for (opcode, expected) in [
            (ScriptOpcode::LoadLastSave, ScriptEvent::LoadLastSave),
            (ScriptOpcode::QuitGame, ScriptEvent::QuitGame),
        ] {
            let mut runtime = ScriptRuntime::new(table(&[
                [0, 0, 0, 0],
                [opcode.raw(), 0, 0, 0],
                [ScriptOpcode::Stop.raw(), 0, 0, 0],
            ]));
            runtime.start(trigger(1));
            assert_eq!(runtime.advance(), Some(expected));
            assert!(!runtime.is_active());
        }
    }

    #[test]
    fn follower_and_scene_map_opcodes_yield_persistent_world_actions() {
        let mut runtime = ScriptRuntime::new(table(&[
            [0, 0, 0, 0],
            [ScriptOpcode::SetPartyFollower.raw(), 3, 4, 0],
            [ScriptOpcode::SetSceneMap.raw(), 0xffff, 17, 0],
            [ScriptOpcode::SetSceneMap.raw(), 5, 18, 0],
            [ScriptOpcode::Stop.raw(), 0, 0, 0],
        ]));
        runtime.start(trigger(1));
        assert_eq!(
            runtime.advance(),
            Some(ScriptEvent::Action(ScriptAction::SetPartyFollowers {
                followers: [Some(3), Some(4)]
            }))
        );
        assert_eq!(
            runtime.advance(),
            Some(ScriptEvent::Action(ScriptAction::SetSceneMap {
                scene_number: None,
                map_number: 17,
            }))
        );
        assert_eq!(
            runtime.advance(),
            Some(ScriptEvent::Action(ScriptAction::SetSceneMap {
                scene_number: Some(5),
                map_number: 18,
            }))
        );
        assert!(matches!(
            runtime.advance(),
            Some(ScriptEvent::Completed { .. })
        ));
    }

    #[test]
    fn teleport_yields_scene_transfer_request_and_preserves_failure_entry() {
        let mut runtime = ScriptRuntime::new(table(&[
            [0, 0, 0, 0],
            [ScriptOpcode::TeleportParty.raw(), 47, 0, 0],
        ]));
        runtime.start(trigger(1));
        assert_eq!(
            runtime.advance(),
            Some(ScriptEvent::Teleport { failure_entry: 47 })
        );
    }

    #[test]
    fn battle_suspends_execution_and_resumes_on_the_result_branch() {
        let mut runtime = ScriptRuntime::new(table(&[
            [0, 0, 0, 0],
            [ScriptOpcode::StartBattle.raw(), 18, 4, 5],
            [ScriptOpcode::Stop.raw(), 0, 0, 0],
            [0, 0, 0, 0],
            [ScriptOpcode::Stop.raw(), 0, 0, 0],
            [ScriptOpcode::Stop.raw(), 0, 0, 0],
        ]));
        runtime.start(trigger(1));
        let request = BattleRequest {
            enemy_team: 18,
            lost_entry: 4,
            flee_entry: 5,
            is_boss: false,
        };
        assert_eq!(runtime.advance(), Some(ScriptEvent::StartBattle(request)));
        assert!(runtime.is_active());
        assert!(runtime.is_waiting_for_battle());
        assert_eq!(runtime.advance(), None);
        assert!(runtime.resolve_battle(BattleResult::Lost));
        assert!(!runtime.is_waiting_for_battle());
        assert!(matches!(
            runtime.advance(),
            Some(ScriptEvent::Completed { .. })
        ));
    }

    #[test]
    fn battle_win_continues_and_zero_flee_operand_marks_a_boss() {
        let mut runtime = ScriptRuntime::new(table(&[
            [0, 0, 0, 0],
            [ScriptOpcode::StartBattle.raw(), 18, 40, 0],
            [ScriptOpcode::Stop.raw(), 0, 0, 0],
        ]));
        runtime.start(trigger(1));
        assert_eq!(
            runtime.advance(),
            Some(ScriptEvent::StartBattle(BattleRequest {
                enemy_team: 18,
                lost_entry: 40,
                flee_entry: 0,
                is_boss: true,
            }))
        );
        assert!(runtime.resolve_battle(BattleResult::Won));
        assert!(matches!(
            runtime.advance(),
            Some(ScriptEvent::Completed { .. })
        ));
        assert!(runtime.start(trigger(1)));
        assert!(matches!(
            runtime.advance(),
            Some(ScriptEvent::StartBattle(_))
        ));
        assert!(runtime.resolve_battle(BattleResult::Terminated));
        assert!(matches!(
            runtime.advance(),
            Some(ScriptEvent::Completed { .. })
        ));
    }

    #[test]
    fn battle_configuration_opcodes_yield_world_actions() {
        let mut runtime = ScriptRuntime::new(table(&[
            [0, 0, 0, 0],
            [ScriptOpcode::SetBattleMusic.raw(), 7, 0, 0],
            [ScriptOpcode::SetBattlefield.raw(), 21, 0, 0],
            [ScriptOpcode::Stop.raw(), 0, 0, 0],
        ]));
        runtime.start(trigger(1));
        assert_eq!(
            runtime.advance(),
            Some(ScriptEvent::Action(ScriptAction::SetBattleMusic {
                music_id: 7,
            }))
        );
        assert_eq!(
            runtime.advance(),
            Some(ScriptEvent::Action(ScriptAction::SetBattlefield {
                battlefield_id: 21,
            }))
        );
    }

    #[test]
    fn nested_script_call_returns_to_the_caller_entry() {
        let mut runtime = ScriptRuntime::new(table(&[
            [0, 0, 0, 0],
            [ScriptOpcode::Stop.raw(), 0, 0, 0],
            [ScriptOpcode::Call.raw(), 3, 0, 0],
            [ScriptOpcode::Stop.raw(), 0, 0, 0],
        ]));
        runtime.start(trigger(2));
        assert!(runtime.call(3, 0xffff));
        assert_eq!(
            runtime.advance(),
            Some(ScriptEvent::Completed {
                trigger: trigger(2),
                next_entry: 2,
                succeeded: true,
            })
        );
    }

    #[test]
    fn debug_snapshot_tracks_trigger_and_instructions_after_completion() {
        let mut runtime =
            ScriptRuntime::new(table(&[[0, 0, 0, 0], [0x0005, 2, 0, 0], [0, 0, 0, 0]]));
        let request = trigger(1);

        assert!(runtime.start(request));
        assert_eq!(
            runtime.debug_snapshot(),
            ScriptDebugSnapshot {
                active: true,
                trigger: Some(request),
                last_instruction: None,
                next_instruction: Some(ScriptInstructionDebug {
                    object_id: request.object_id,
                    entry: 1,
                    opcode: 0x0005,
                    operands: [2, 0, 0],
                }),
                call_depth: 0,
                wait_frames: 0,
            }
        );

        assert_eq!(runtime.advance(), Some(ScriptEvent::Delay));
        let waiting = runtime.debug_snapshot();
        assert_eq!(waiting.last_instruction.unwrap().entry, 1);
        assert_eq!(waiting.next_instruction.unwrap().entry, 2);
        assert_eq!(waiting.wait_frames, 1);

        assert_eq!(runtime.advance(), Some(ScriptEvent::Delay));
        assert!(matches!(
            runtime.advance(),
            Some(ScriptEvent::Completed { .. })
        ));
        let completed = runtime.debug_snapshot();
        assert!(!completed.active);
        assert_eq!(completed.trigger, Some(request));
        assert_eq!(completed.last_instruction.unwrap().entry, 2);
        assert_eq!(completed.next_instruction, None);
    }

    fn trigger(entry: u16) -> TriggerRequest {
        TriggerRequest {
            object_id: 7,
            script_entry: entry,
            kind: TriggerKind::Search,
        }
    }

    #[test]
    fn yields_messages_and_resumes_until_completion() {
        let mut runtime = ScriptRuntime::new(table(&[
            [0, 0, 0, 0],
            [0x003c, 0, 0, 0],
            [0xffff, 42, 0, 0],
            [0xffff, 43, 0, 0],
            [0, 0, 0, 0],
        ]));
        assert!(runtime.start(trigger(1)));
        assert_eq!(
            runtime.advance(),
            Some(ScriptEvent::Message {
                message_id: 42,
                position: DialogPosition::Upper,
                font_color: 0x4f,
                face_index: None,
            })
        );
        assert_eq!(
            runtime.advance(),
            Some(ScriptEvent::Message {
                message_id: 43,
                position: DialogPosition::Upper,
                font_color: 0x4f,
                face_index: None,
            })
        );
        assert_eq!(
            runtime.advance(),
            Some(ScriptEvent::Completed {
                trigger: trigger(1),
                next_entry: 1,
                succeeded: true,
            })
        );
        assert!(!runtime.is_active());
    }

    #[test]
    fn yields_item_recovery_actions_and_reports_script_success() {
        let mut runtime = ScriptRuntime::new(table(&[
            [0, 0, 0, 0],
            [0x001b, 0, 50, 0],
            [0x001c, 1, 20, 0],
            [0x001d, 0, 10, 0],
            [0x0022, 0, 3, 0],
            [0, 0, 0, 0],
        ]));
        runtime.start(trigger(1));
        assert_eq!(
            runtime.advance(),
            Some(ScriptEvent::Action(ScriptAction::AdjustPlayerHealth {
                role_id: 7,
                hp: 50,
                mp: 0,
                apply_to_all: false,
            }))
        );
        assert_eq!(
            runtime.advance(),
            Some(ScriptEvent::Action(ScriptAction::AdjustPlayerHealth {
                role_id: 7,
                hp: 0,
                mp: 20,
                apply_to_all: true,
            }))
        );
        assert_eq!(
            runtime.advance(),
            Some(ScriptEvent::Action(ScriptAction::AdjustPlayerHealth {
                role_id: 7,
                hp: 10,
                mp: 10,
                apply_to_all: false,
            }))
        );
        assert_eq!(
            runtime.advance(),
            Some(ScriptEvent::Action(ScriptAction::RevivePlayer {
                role_id: 7,
                hp_tenths: 3,
                apply_to_all: false,
            }))
        );
        assert!(runtime.set_success(false));
        assert_eq!(
            runtime.advance(),
            Some(ScriptEvent::Completed {
                trigger: trigger(1),
                next_entry: 1,
                succeeded: false,
            })
        );
    }

    #[test]
    fn yields_battle_status_and_poison_actions() {
        let mut runtime = ScriptRuntime::new(table(&[
            [0, 0, 0, 0],
            [ScriptOpcode::DamageEnemy.raw(), 1, 25, 0],
            [ScriptOpcode::PoisonEnemy.raw(), 0, 40, 0],
            [ScriptOpcode::PoisonPlayer.raw(), 1, 41, 0],
            [ScriptOpcode::CureEnemyPoison.raw(), 0, 40, 0],
            [ScriptOpcode::CurePlayerPoison.raw(), 1, 41, 0],
            [ScriptOpcode::CurePoisonByLevel.raw(), 0, 3, 0],
            [ScriptOpcode::SetPlayerStatus.raw(), 6, 4, 0],
            [ScriptOpcode::SetEnemyStatus.raw(), 2, 5, 99],
            [ScriptOpcode::RemovePlayerStatus.raw(), 6, 0, 0],
            [ScriptOpcode::AdjustTemporaryPlayerStat.raw(), 17, 50, 2],
            [ScriptOpcode::SetTemporaryBattleSprite.raw(), 5, 0, 0],
            [ScriptOpcode::CollectEnemy.raw(), 88, 0, 0],
            [ScriptOpcode::TransmuteCollectedEnemies.raw(), 0, 0, 0],
            [ScriptOpcode::HideBattleActor.raw(), 3, 0, 0],
            [ScriptOpcode::StealEnemy.raw(), 5, 0, 0],
            [ScriptOpcode::EnableAutoBattle.raw(), 0, 0, 0],
            [ScriptOpcode::DrainEnemyHp.raw(), 12, 0, 0],
            [ScriptOpcode::FleeBattle.raw(), 94, 0, 0],
            [ScriptOpcode::HalvePlayerHp.raw(), 0, 0, 0],
            [ScriptOpcode::HalveEnemyHp.raw(), 50, 0, 0],
            [ScriptOpcode::KillPlayer.raw(), 0, 0, 0],
            [ScriptOpcode::KillEnemy.raw(), 0, 0, 0],
            [ScriptOpcode::EnemyCastMagic.raw(), 88, 0, 0],
            [ScriptOpcode::EnemyEscape.raw(), 0, 0, 0],
            [ScriptOpcode::SetBattleResult.raw(), 0, 0, 0],
            [ScriptOpcode::SimulatePlayerMagic.raw(), 88, 123, 0],
            [ScriptOpcode::ThrowWeapon.raw(), 91, 7, 0],
            [ScriptOpcode::ScaleMagicByMp.raw(), 89, 0, 0],
            [ScriptOpcode::ScaleMagicByCash.raw(), 90, 0, 0],
            [ScriptOpcode::DivideEnemy.raw(), 2, 98, 0],
            [ScriptOpcode::SummonEnemy.raw(), 501, 3, 99],
            [ScriptOpcode::TransformEnemy.raw(), 502, 0, 0],
            [ScriptOpcode::JumpIfPlayerLacksPoison.raw(), 40, 91, 0],
            [ScriptOpcode::JumpIfEnemyLacksPoison.raw(), 41, 92, 0],
            [ScriptOpcode::JumpIfPlayerNotPoisoned.raw(), 93, 0, 0],
            [ScriptOpcode::JumpIfEnemyHpAbove.raw(), 60, 95, 0],
            [ScriptOpcode::JumpIfEnemyNotFirstKind.raw(), 96, 0, 0],
            [ScriptOpcode::JumpIfEnemyTurn.raw(), 97, 0, 0],
            [ScriptOpcode::Stop.raw(), 0, 0, 0],
        ]));
        runtime.start(trigger(1));
        let expected = [
            ScriptAction::DamageEnemy {
                enemy_index: 7,
                amount: 25,
                apply_to_all: true,
            },
            ScriptAction::PoisonEnemy {
                enemy_index: 7,
                poison_id: 40,
                apply_to_all: false,
            },
            ScriptAction::PoisonPlayer {
                role_id: 7,
                poison_id: 41,
                apply_to_all: true,
            },
            ScriptAction::CureEnemyPoison {
                enemy_index: 7,
                poison_id: 40,
                apply_to_all: false,
            },
            ScriptAction::CurePlayerPoison {
                role_id: 7,
                poison_id: 41,
                apply_to_all: true,
            },
            ScriptAction::CurePlayerPoisonByLevel {
                role_id: 7,
                maximum_level: 3,
                apply_to_all: false,
            },
            ScriptAction::SetPlayerStatus {
                role_id: 7,
                status: 6,
                rounds: 4,
            },
            ScriptAction::SetEnemyStatus {
                enemy_index: 7,
                status: 2,
                rounds: 5,
                resisted_entry: 99,
            },
            ScriptAction::RemovePlayerStatus {
                role_id: 7,
                status: 6,
            },
            ScriptAction::AdjustTemporaryPlayerStat {
                role_id: 1,
                attribute: 17,
                percent: 50,
            },
            ScriptAction::SetTemporaryBattleSprite {
                role_id: 7,
                sprite: 5,
            },
            ScriptAction::CollectEnemy {
                enemy_index: 7,
                failure_entry: 88,
            },
            ScriptAction::TransmuteCollectedEnemies,
            ScriptAction::HideBattleActor { rounds: 3 },
            ScriptAction::StealEnemy {
                enemy_index: 7,
                rate: 5,
            },
            ScriptAction::EnableAutoBattle,
            ScriptAction::DrainEnemyHp {
                enemy_index: 7,
                amount: 12,
            },
            ScriptAction::FleeBattle { failure_entry: 94 },
            ScriptAction::HalvePlayerHp { role_id: 7 },
            ScriptAction::HalveEnemyHp {
                enemy_index: 7,
                maximum_damage: 50,
            },
            ScriptAction::KillPlayer { role_id: 7 },
            ScriptAction::KillEnemy { enemy_index: 7 },
            ScriptAction::SetEnemyMagic {
                enemy_index: 7,
                magic_object: 88,
                rate: 0,
            },
            ScriptAction::EnemyEscape,
            ScriptAction::SetBattleResult { result: 0 },
            ScriptAction::SimulatePlayerMagic {
                enemy_index: 7,
                magic_object: 88,
                base_strength: 123,
            },
            ScriptAction::ThrowWeapon {
                enemy_index: 7,
                magic_object: 91,
                multiplier: 7,
            },
            ScriptAction::ScaleMagicByMp {
                role_id: 7,
                magic_object: 89,
                multiplier: 8,
            },
            ScriptAction::ScaleMagicByCash { magic_object: 90 },
            ScriptAction::DivideEnemy {
                enemy_index: 7,
                copies: 2,
                failure_entry: 98,
            },
            ScriptAction::SummonEnemy {
                enemy_index: 7,
                object_id: 501,
                count: 3,
                failure_entry: 99,
            },
            ScriptAction::TransformEnemy {
                enemy_index: 7,
                object_id: 502,
            },
        ];
        for action in expected {
            assert_eq!(runtime.advance(), Some(ScriptEvent::Action(action)));
        }
        assert_eq!(
            runtime.advance(),
            Some(ScriptEvent::Condition(ScriptCondition::PlayerLacksPoison {
                role_id: 7,
                poison_id: 40,
                target_entry: 91,
            }))
        );
        assert_eq!(
            runtime.advance(),
            Some(ScriptEvent::Condition(ScriptCondition::EnemyLacksPoison {
                enemy_index: 7,
                poison_id: 41,
                target_entry: 92,
            }))
        );
        assert_eq!(
            runtime.advance(),
            Some(ScriptEvent::Condition(ScriptCondition::PlayerNotPoisoned {
                role_id: 7,
                target_entry: 93,
            }))
        );
        assert_eq!(
            runtime.advance(),
            Some(ScriptEvent::Condition(ScriptCondition::EnemyHpAbove {
                enemy_index: 7,
                percentage: 60,
                target_entry: 95,
            }))
        );
        assert_eq!(
            runtime.advance(),
            Some(ScriptEvent::Condition(ScriptCondition::EnemyNotFirstKind {
                enemy_index: 7,
                target_entry: 96,
            }))
        );
        assert_eq!(
            runtime.advance(),
            Some(ScriptEvent::Condition(ScriptCondition::EnemyTurn {
                target_entry: 97,
            }))
        );
        assert!(matches!(
            runtime.advance(),
            Some(ScriptEvent::Completed { .. })
        ));
    }

    #[test]
    fn yields_chase_growth_and_cash_actions() {
        let mut runtime = ScriptRuntime::new(table(&[
            [0, 0, 0, 0],
            [ScriptOpcode::PauseEnemyChase.raw(), 30, 0, 0],
            [ScriptOpcode::SpeedUpEnemyChase.raw(), 40, 0, 0],
            [ScriptOpcode::LevelUpPlayer.raw(), 2, 0, 0],
            [ScriptOpcode::HalveCash.raw(), 0, 0, 0],
            [ScriptOpcode::Stop.raw(), 0, 0, 0],
        ]));
        runtime.start(trigger(1));
        for expected in [
            ScriptAction::SetEnemyChase {
                range: 0,
                cycles: 30,
            },
            ScriptAction::SetEnemyChase {
                range: 3,
                cycles: 40,
            },
            ScriptAction::LevelUpPlayer {
                role_id: 7,
                levels: 2,
            },
            ScriptAction::HalveCash,
        ] {
            assert_eq!(runtime.advance(), Some(ScriptEvent::Action(expected)));
        }
        assert!(matches!(
            runtime.advance(),
            Some(ScriptEvent::Completed { .. })
        ));
    }

    #[test]
    fn yields_role_sprite_zone_check_and_cd_fallback_actions() {
        let mut runtime = ScriptRuntime::new(table(&[
            [0, 0, 0, 0],
            [ScriptOpcode::SetPlayerSprite.raw(), 3, 42, 1],
            [ScriptOpcode::JumpIfObjectOutsideZone.raw(), 9, 2, 77],
            [ScriptOpcode::PlayCdMusic.raw(), 5, 31, 0],
            [ScriptOpcode::Stop.raw(), 0, 0, 0],
        ]));
        runtime.start(trigger(1));
        assert_eq!(
            runtime.advance(),
            Some(ScriptEvent::Action(ScriptAction::SetPlayerSprite {
                role_id: 3,
                sprite_index: 42,
                reload: true,
            }))
        );
        assert_eq!(
            runtime.advance(),
            Some(ScriptEvent::Action(ScriptAction::CheckObjectZone {
                object_id: 7,
                target_id: 9,
                range: 2,
                failure_entry: 77,
            }))
        );
        assert_eq!(
            runtime.advance(),
            Some(ScriptEvent::Action(ScriptAction::PlayMusic {
                music_id: 31,
                looped: true,
                fade_seconds: 0,
            }))
        );
        assert!(matches!(
            runtime.advance(),
            Some(ScriptEvent::Completed { .. })
        ));
    }

    #[test]
    fn dialog_opcodes_preserve_face_and_font_color() {
        let mut runtime = ScriptRuntime::new(table(&[
            [0, 0, 0, 0],
            [0x003c, 5, 0x2d, 0],
            [0xffff, 42, 0, 0],
            [0x003d, 6, 0x1a, 0],
            [0xffff, 43, 0, 0],
        ]));
        runtime.start(trigger(1));
        assert_eq!(
            runtime.advance(),
            Some(ScriptEvent::Message {
                message_id: 42,
                position: DialogPosition::Upper,
                font_color: 0x2d,
                face_index: Some(5),
            })
        );
        assert_eq!(
            runtime.advance(),
            Some(ScriptEvent::Message {
                message_id: 43,
                position: DialogPosition::Lower,
                font_color: 0x1a,
                face_index: Some(6),
            })
        );
    }

    #[test]
    fn follows_jumps_and_updates_persistent_entry() {
        let mut runtime = ScriptRuntime::new(table(&[
            [0, 0, 0, 0],
            [3, 3, 0, 0],
            [0, 0, 0, 0],
            [8, 0, 0, 0],
            [0, 0, 0, 0],
        ]));
        runtime.start(trigger(1));
        assert_eq!(
            runtime.advance(),
            Some(ScriptEvent::Completed {
                trigger: trigger(1),
                next_entry: 4,
                succeeded: true,
            })
        );
    }

    #[test]
    fn idle_limited_control_flow_persists_across_trigger_runs() {
        let mut runtime = ScriptRuntime::new(table(&[
            [0, 0, 0, 0],
            [0x0002, 4, 2, 0],
            [0xffff, 9, 0, 0],
            [0, 0, 0, 0],
            [0xffff, 10, 0, 0],
            [0, 0, 0, 0],
        ]));
        runtime.start(trigger(1));
        assert_eq!(
            runtime.advance(),
            Some(ScriptEvent::Completed {
                trigger: trigger(1),
                next_entry: 4,
                succeeded: true,
            })
        );

        runtime.start(trigger(1));
        assert_eq!(
            runtime.advance(),
            Some(ScriptEvent::Message {
                message_id: 9,
                position: DialogPosition::Lower,
                font_color: 0x4f,
                face_index: None,
            })
        );

        let mut jump = ScriptRuntime::new(table(&[
            [0, 0, 0, 0],
            [0x0003, 1, 2, 0],
            [0xffff, 20, 0, 0],
            [0, 0, 0, 0],
        ]));
        jump.start(trigger(1));
        assert_eq!(
            jump.advance(),
            Some(ScriptEvent::Message {
                message_id: 20,
                position: DialogPosition::Lower,
                font_color: 0x4f,
                face_index: None,
            })
        );
    }

    #[test]
    fn calls_subscripts_and_returns_to_the_caller() {
        let mut runtime = ScriptRuntime::new(table(&[
            [0, 0, 0, 0],
            [0x0004, 4, 9, 0],
            [0xffff, 42, 0, 0],
            [0, 0, 0, 0],
            [0x0014, 2, 0, 0],
            [0, 0, 0, 0],
        ]));
        runtime.start(trigger(1));
        assert_eq!(
            runtime.advance(),
            Some(ScriptEvent::Action(ScriptAction::SetObjectPose {
                object_id: 9,
                direction: Some(Direction::South),
                frame: Some(2),
            }))
        );
        assert_eq!(
            runtime.advance(),
            Some(ScriptEvent::Message {
                message_id: 42,
                position: DialogPosition::Lower,
                font_color: 0x4f,
                face_index: None,
            })
        );
        assert!(matches!(
            runtime.advance(),
            Some(ScriptEvent::Completed { .. })
        ));
    }

    #[test]
    fn subscript_persistent_exit_does_not_mutate_the_called_object() {
        let mut runtime = ScriptRuntime::new(table(&[
            [0, 0, 0, 0],
            [0x0004, 4, 9, 0],
            [0xffff, 42, 0, 0],
            [0, 0, 0, 0],
            [0x0001, 0, 0, 0],
            [0, 0, 0, 0],
        ]));
        runtime.start(trigger(1));
        assert_eq!(
            runtime.advance(),
            Some(ScriptEvent::Message {
                message_id: 42,
                position: DialogPosition::Lower,
                font_color: 0x4f,
                face_index: None,
            })
        );
    }

    #[test]
    fn probability_branch_uses_a_deterministic_percent_roll() {
        let mut runtime = ScriptRuntime::new(table(&[
            [0, 0, 0, 0],
            [0x0006, 1, 3, 0],
            [0xffff, 10, 0, 0],
            [0xffff, 20, 0, 0],
            [0, 0, 0, 0],
        ]));
        runtime.start(trigger(1));
        assert_eq!(
            runtime.advance(),
            Some(ScriptEvent::Message {
                message_id: 20,
                position: DialogPosition::Lower,
                font_color: 0x4f,
                face_index: None,
            })
        );

        let mut impossible = ScriptRuntime::new(table(&[
            [0, 0, 0, 0],
            [0x0006, 101, 3, 0],
            [0xffff, 30, 0, 0],
            [0xffff, 40, 0, 0],
        ]));
        impossible.start(trigger(1));
        assert_eq!(
            impossible.advance(),
            Some(ScriptEvent::Message {
                message_id: 30,
                position: DialogPosition::Lower,
                font_color: 0x4f,
                face_index: None,
            })
        );
    }

    #[test]
    fn redraw_delay_does_not_report_a_scene_updating_wait() {
        let mut runtime = ScriptRuntime::new(table(&[
            [0, 0, 0, 0],
            [0x0005, 0, 0, 0],
            [0xffff, 12, 0, 0],
        ]));
        runtime.start(trigger(1));
        assert_eq!(runtime.advance(), Some(ScriptEvent::Delay));
        assert_eq!(runtime.advance(), Some(ScriptEvent::Delay));
        assert_eq!(
            runtime.advance(),
            Some(ScriptEvent::Message {
                message_id: 12,
                position: DialogPosition::Lower,
                font_color: 0x4f,
                face_index: None,
            })
        );
    }

    #[test]
    fn yields_repeating_walk_and_relative_position_actions() {
        let mut runtime = ScriptRuntime::new(table(&[
            [0, 0, 0, 0],
            [0x0010, 4, 6, 1],
            [0x0012, 9, 0xfff0, 8],
            [0, 0, 0, 0],
        ]));
        runtime.start(trigger(1));
        assert_eq!(
            runtime.advance(),
            Some(ScriptEvent::Action(ScriptAction::WalkObjectTo {
                object_id: 7,
                tile_x: 4,
                tile_y: 6,
                half: 1,
                speed: 3,
                repeat_entry: 1,
            }))
        );
        assert_eq!(
            runtime.advance(),
            Some(ScriptEvent::Action(
                ScriptAction::SetObjectPositionRelativeToPlayer {
                    object_id: 9,
                    dx: -16,
                    dy: 8,
                }
            ))
        );
    }

    #[test]
    fn moves_viewport_over_multiple_script_ticks() {
        let mut runtime =
            ScriptRuntime::new(table(&[[0, 0, 0, 0], [0x007f, 2, 0xffff, 3], [0, 0, 0, 0]]));
        runtime.start(trigger(1));
        for _ in 0..3 {
            assert_eq!(
                runtime.advance(),
                Some(ScriptEvent::Action(ScriptAction::MoveViewport {
                    x: 2,
                    y: -1,
                    frames: 1,
                }))
            );
        }
        assert!(matches!(
            runtime.advance(),
            Some(ScriptEvent::Completed { .. })
        ));
    }

    #[test]
    fn sets_and_restores_viewport_without_repeating() {
        let mut runtime = ScriptRuntime::new(table(&[
            [0, 0, 0, 0],
            [0x007f, 20, 30, 0xffff],
            [0x007f, 0, 0, 0],
            [0, 0, 0, 0],
        ]));
        runtime.start(trigger(1));
        assert_eq!(
            runtime.advance(),
            Some(ScriptEvent::Action(ScriptAction::MoveViewport {
                x: 20,
                y: 30,
                frames: -1,
            }))
        );
        assert_eq!(
            runtime.advance(),
            Some(ScriptEvent::Action(ScriptAction::MoveViewport {
                x: 0,
                y: 0,
                frames: 1,
            }))
        );
        assert!(matches!(
            runtime.advance(),
            Some(ScriptEvent::Completed { .. })
        ));

        let mut single_frame =
            ScriptRuntime::new(table(&[[0, 0, 0, 0], [0x007f, 1, 0, 0], [0, 0, 0, 0]]));
        single_frame.start(trigger(1));
        assert_eq!(
            single_frame.advance(),
            Some(ScriptEvent::Action(ScriptAction::MoveViewport {
                x: 1,
                y: 0,
                frames: 1,
            }))
        );
        assert!(matches!(
            single_frame.advance(),
            Some(ScriptEvent::Completed { .. })
        ));
    }

    #[test]
    fn yields_party_ride_speeds_and_collapse_action() {
        let mut runtime = ScriptRuntime::new(table(&[
            [0, 0, 0, 0],
            [0x003f, 10, 20, 0],
            [0x0044, 11, 21, 1],
            [0x0097, 12, 22, 0],
            [0x0078, 0, 0, 0],
            [0x00a1, 0, 0, 0],
            [0, 0, 0, 0],
        ]));
        runtime.start(trigger(1));
        for (entry, speed) in [(1, 2), (2, 4), (3, 8)] {
            assert!(matches!(
                runtime.advance(),
                Some(ScriptEvent::Action(ScriptAction::RideObjectTo {
                    object_id: 7,
                    speed: actual_speed,
                    repeat_entry,
                    ..
                })) if actual_speed == speed && repeat_entry == entry
            ));
        }
        assert_eq!(
            runtime.advance(),
            Some(ScriptEvent::Action(ScriptAction::CollapseParty))
        );
        assert!(matches!(
            runtime.advance(),
            Some(ScriptEvent::Completed { .. })
        ));
    }

    #[test]
    fn yields_confirmation_and_item_removal() {
        let mut runtime = ScriptRuntime::new(table(&[
            [0, 0, 0, 0],
            [0x000a, 4, 0, 0],
            [0x0020, 99, 0, 6],
            [0, 0, 0, 0],
            [0xffff, 10, 0, 0],
            [0, 0, 0, 0],
            [0xffff, 11, 0, 0],
            [0, 0, 0, 0],
        ]));
        runtime.start(trigger(1));
        assert_eq!(
            runtime.advance(),
            Some(ScriptEvent::Confirm { no_entry: 4 })
        );
        assert_eq!(
            runtime.advance(),
            Some(ScriptEvent::Action(ScriptAction::RemoveItem {
                item_id: 99,
                amount: 1,
                insufficient_entry: 6,
            }))
        );
        assert!(runtime.branch_to(6));
        assert_eq!(
            runtime.advance(),
            Some(ScriptEvent::Message {
                message_id: 11,
                position: DialogPosition::Lower,
                font_color: 0x4f,
                face_index: None,
            })
        );
    }

    #[test]
    fn marks_failure_and_yields_party_and_equipment_conditions() {
        let mut runtime = ScriptRuntime::new(table(&[
            [0, 0, 0, 0],
            [0x0084, 798, 2, 7],
            [0x0041, 0, 0, 0],
            [0x0074, 8, 0, 0],
            [0x0086, 274, 2, 9],
            [0, 0, 0, 0],
        ]));
        runtime.start(trigger(1));
        assert_eq!(
            runtime.advance(),
            Some(ScriptEvent::Action(ScriptAction::PlaceObjectInFront {
                object_id: 798,
                state: 2,
                blocked_entry: 7,
            }))
        );
        assert_eq!(
            runtime.advance(),
            Some(ScriptEvent::Condition(ScriptCondition::PartyNotFullHp {
                target_entry: 8,
            }))
        );
        assert_eq!(
            runtime.advance(),
            Some(ScriptEvent::Condition(ScriptCondition::ItemNotEquipped {
                item_id: 274,
                amount: 2,
                target_entry: 9,
            }))
        );
        assert_eq!(
            runtime.advance(),
            Some(ScriptEvent::Completed {
                trigger: trigger(1),
                next_entry: 1,
                succeeded: false,
            })
        );
    }

    #[test]
    fn yields_equipment_attribute_and_magic_actions() {
        let mut runtime = ScriptRuntime::new(table(&[
            [0, 0, 0, 0],
            [0x0018, 0x0e, 163, 0],
            [0x0017, 0x0e, 17, 20],
            [0x001a, 4, 1, 0],
            [0x0019, 17, 3, 2],
            [0x0023, 1, 4, 0],
            [0x0055, 88, 0, 0],
            [0x0056, 89, 2, 0],
            [0, 0, 0, 0],
        ]));
        runtime.start(trigger(1));
        assert_eq!(
            runtime.advance(),
            Some(ScriptEvent::Action(ScriptAction::EquipItem {
                role_id: 7,
                slot: 3,
                item_id: 163,
            }))
        );
        assert_eq!(
            runtime.advance(),
            Some(ScriptEvent::Action(ScriptAction::SetEquipmentEffect {
                role_id: 7,
                attribute: 17,
                slot: 3,
                value: 20,
            }))
        );
        assert_eq!(
            runtime.advance(),
            Some(ScriptEvent::Action(ScriptAction::ChangePlayerAttribute {
                role_id: 7,
                attribute: 4,
                value: 1,
                absolute: true,
            }))
        );
        assert!(matches!(
            runtime.advance(),
            Some(ScriptEvent::Action(ScriptAction::ChangePlayerAttribute {
                role_id: 1,
                absolute: false,
                ..
            }))
        ));
        assert_eq!(
            runtime.advance(),
            Some(ScriptEvent::Action(ScriptAction::RemoveEquipment {
                role_id: 1,
                slot: Some(3),
            }))
        );
        assert_eq!(
            runtime.advance(),
            Some(ScriptEvent::Action(ScriptAction::ChangeMagic {
                role_id: 7,
                magic_id: 88,
                add: true,
            }))
        );
        assert_eq!(
            runtime.advance(),
            Some(ScriptEvent::Action(ScriptAction::ChangeMagic {
                role_id: 1,
                magic_id: 89,
                add: false,
            }))
        );
    }

    #[test]
    fn yields_buy_and_sell_menus() {
        let mut runtime = ScriptRuntime::new(table(&[
            [0, 0, 0, 0],
            [0x0026, 3, 0, 0],
            [0x0027, 0, 0, 0],
            [0, 0, 0, 0],
        ]));
        runtime.start(trigger(1));
        assert_eq!(
            runtime.advance(),
            Some(ScriptEvent::OpenBuyMenu { store_number: 3 })
        );
        assert_eq!(runtime.advance(), Some(ScriptEvent::OpenSellMenu));
        assert!(matches!(
            runtime.advance(),
            Some(ScriptEvent::Completed { .. })
        ));
    }

    #[test]
    fn yields_player_facing_condition() {
        let mut runtime =
            ScriptRuntime::new(table(&[[0, 0, 0, 0], [0x0081, 12, 2, 7], [0, 0, 0, 0]]));
        runtime.start(trigger(1));
        assert_eq!(
            runtime.advance(),
            Some(ScriptEvent::Condition(ScriptCondition::PlayerFacesObject {
                object_id: 12,
                range: 2,
                target_entry: 7,
            }))
        );
    }

    #[test]
    fn yields_scene_script_updates() {
        let mut runtime = ScriptRuntime::new(table(&[
            [0, 0, 0, 0],
            [0x006d, 2, 100, 200],
            [0x006d, 3, 0, 0],
            [0, 0, 0, 0],
        ]));
        runtime.start(trigger(1));
        assert_eq!(
            runtime.advance(),
            Some(ScriptEvent::Action(ScriptAction::SetSceneScripts {
                scene_number: 2,
                enter_script: Some(100),
                teleport_script: Some(200),
            }))
        );
        assert_eq!(
            runtime.advance(),
            Some(ScriptEvent::Action(ScriptAction::SetSceneScripts {
                scene_number: 3,
                enter_script: Some(0),
                teleport_script: Some(0),
            }))
        );
    }

    #[test]
    fn reports_unsupported_and_invalid_entries() {
        let mut runtime = ScriptRuntime::new(table(&[
            [0, 0, 0, 0],
            [ScriptOpcode::BlowEnemiesAway.raw(), 0, 0, 0],
        ]));
        runtime.start(trigger(1));
        assert_eq!(
            runtime.advance(),
            Some(ScriptEvent::Unsupported {
                trigger: trigger(1),
                entry: 1,
                opcode: ScriptOpcode::BlowEnemiesAway.raw(),
            })
        );

        runtime.start(trigger(99));
        assert_eq!(
            runtime.advance(),
            Some(ScriptEvent::InvalidEntry {
                trigger: trigger(99),
                entry: 99,
            })
        );
    }

    #[test]
    fn yields_wait_ticks_and_world_actions() {
        let mut runtime = ScriptRuntime::new(table(&[
            [0, 0, 0, 0],
            [0x0009, 2, 0, 0],
            [0x000b, 0, 0, 0],
            [0x0049, 0xffff, 0xffff, 0],
            [0x006e, 0xfff0, 8, 0],
            [0, 0, 0, 0],
        ]));
        runtime.start(trigger(1));
        assert_eq!(runtime.advance(), Some(ScriptEvent::Waiting));
        assert_eq!(runtime.advance(), Some(ScriptEvent::Waiting));
        assert_eq!(
            runtime.advance(),
            Some(ScriptEvent::Action(ScriptAction::MoveObject {
                object_id: 7,
                direction: Direction::South,
            }))
        );
        assert_eq!(
            runtime.advance(),
            Some(ScriptEvent::Action(ScriptAction::SetObjectState {
                object_id: 7,
                state: -1,
            }))
        );
        assert_eq!(
            runtime.advance(),
            Some(ScriptEvent::Action(ScriptAction::OffsetPlayer {
                dx: -16,
                dy: 8,
            }))
        );
        assert!(matches!(
            runtime.advance(),
            Some(ScriptEvent::Completed { .. })
        ));
    }

    #[test]
    fn updates_object_trigger_fields_and_delays_in_80ms_periods() {
        let mut runtime = ScriptRuntime::new(table(&[
            [0, 0, 0, 0],
            [0x0024, 10, 0x135d, 0],
            [0x0040, 10, 2, 0],
            [0x0025, 10, 0x119e, 0],
            [0x0085, 2, 0, 0],
            [0, 0, 0, 0],
        ]));
        runtime.start(trigger(1));
        assert_eq!(
            runtime.advance(),
            Some(ScriptEvent::Action(ScriptAction::SetObjectAutoScript {
                object_id: 10,
                script_entry: 0x135d,
            }))
        );
        assert_eq!(
            runtime.advance(),
            Some(ScriptEvent::Action(ScriptAction::SetObjectTriggerMode {
                object_id: 10,
                trigger_mode: 2,
            }))
        );
        assert_eq!(
            runtime.advance(),
            Some(ScriptEvent::Action(ScriptAction::SetObjectTriggerScript {
                object_id: 10,
                script_entry: 0x119e,
            }))
        );
        assert_eq!(runtime.advance(), Some(ScriptEvent::Delay));
        assert_eq!(runtime.advance(), Some(ScriptEvent::Delay));
        assert_eq!(runtime.advance(), Some(ScriptEvent::Delay));
        assert_eq!(runtime.advance(), Some(ScriptEvent::Delay));
        assert!(matches!(
            runtime.advance(),
            Some(ScriptEvent::Completed { .. })
        ));
    }

    #[test]
    fn yields_inventory_position_and_scene_actions() {
        let mut runtime = ScriptRuntime::new(table(&[
            [0, 0, 0, 0],
            [0x001f, 99, 0, 0],
            [0x0046, 45, 96, 0],
            [0x0059, 3, 0, 0],
            [0, 0, 0, 0],
        ]));
        runtime.start(trigger(1));
        assert_eq!(
            runtime.advance(),
            Some(ScriptEvent::Action(ScriptAction::AddItem {
                item_id: 99,
                amount: 0,
            }))
        );
        assert_eq!(
            runtime.advance(),
            Some(ScriptEvent::Action(ScriptAction::SetPlayerPosition {
                tile_x: 45,
                tile_y: 96,
                half: 0,
            }))
        );
        assert_eq!(
            runtime.advance(),
            Some(ScriptEvent::Action(ScriptAction::ChangeScene {
                scene_number: 3,
            }))
        );
    }

    #[test]
    fn yields_zero_based_party_members_and_default_leader() {
        let mut runtime = ScriptRuntime::new(table(&[
            [0, 0, 0, 0],
            [0x0075, 3, 1, 0],
            [0x0075, 0, 0, 0],
            [0, 0, 0, 0],
        ]));
        runtime.start(trigger(1));
        assert_eq!(
            runtime.advance(),
            Some(ScriptEvent::Action(ScriptAction::SetParty {
                members: [Some(2), Some(0), None],
            }))
        );
        assert_eq!(
            runtime.advance(),
            Some(ScriptEvent::Action(ScriptAction::SetParty {
                members: [Some(0), None, None],
            }))
        );
    }

    #[test]
    fn yields_scene_fade_and_continues_at_the_next_instruction() {
        let mut runtime = ScriptRuntime::new(table(&[
            [0, 0, 0, 0],
            [0x0073, 4, 0x48, 0],
            [0xffff, 42, 0, 0],
            [0, 0, 0, 0],
        ]));
        runtime.start(trigger(1));
        assert_eq!(runtime.advance(), Some(ScriptEvent::FadeScene { speed: 4 }));
        assert_eq!(
            runtime.advance(),
            Some(ScriptEvent::Message {
                message_id: 42,
                position: DialogPosition::Lower,
                font_color: 0x4f,
                face_index: None,
            })
        );
    }

    #[test]
    fn yields_music_and_sound_actions() {
        let mut runtime = ScriptRuntime::new(table(&[
            [0, 0, 0, 0],
            [0x0043, 7, 3, 0],
            [0x0043, 9, 1, 0],
            [0x0047, 12, 0, 0],
            [0x0077, 0, 0, 0],
            [0, 0, 0, 0],
        ]));
        runtime.start(trigger(1));
        assert_eq!(
            runtime.advance(),
            Some(ScriptEvent::Action(ScriptAction::PlayMusic {
                music_id: 7,
                looped: true,
                fade_seconds: 3,
            }))
        );
        assert_eq!(
            runtime.advance(),
            Some(ScriptEvent::Action(ScriptAction::PlayMusic {
                music_id: 9,
                looped: false,
                fade_seconds: 0,
            }))
        );
        assert_eq!(
            runtime.advance(),
            Some(ScriptEvent::Action(ScriptAction::PlaySound {
                sound_id: 12
            }))
        );
        assert_eq!(
            runtime.advance(),
            Some(ScriptEvent::Action(ScriptAction::PlayMusic {
                music_id: 0,
                looped: false,
                fade_seconds: 2,
            }))
        );
    }

    #[test]
    fn yields_temporary_hide_fast_walk_and_object_transform_actions() {
        let mut runtime = ScriptRuntime::new(table(&[
            [0, 0, 0, 0],
            [0x0052, 0, 0, 0],
            [0x0079, 37, 9, 0],
            [0x007a, 4, 6, 1],
            [0x007c, 8, 9, 0],
            [0x007d, 0xffff, 0xfffc, 2],
            [0x007e, 12, 0xfff6, 0],
            [0x0082, 10, 11, 1],
            [0, 0, 0, 0],
        ]));
        runtime.start(trigger(1));
        assert_eq!(
            runtime.advance(),
            Some(ScriptEvent::Action(ScriptAction::HideObjectTemporarily {
                object_id: 7,
                vanish_time: 800,
            }))
        );
        assert_eq!(
            runtime.advance(),
            Some(ScriptEvent::Condition(ScriptCondition::PartyContainsName {
                name_word_id: 37,
                target_entry: 9,
            }))
        );
        assert!(matches!(
            runtime.advance(),
            Some(ScriptEvent::Action(ScriptAction::WalkPlayerTo {
                speed: 4,
                repeat_entry: 3,
                ..
            }))
        ));
        assert!(matches!(
            runtime.advance(),
            Some(ScriptEvent::Action(ScriptAction::WalkObjectTo {
                object_id: 7,
                speed: 2,
                repeat_entry: 4,
                ..
            }))
        ));
        assert_eq!(
            runtime.advance(),
            Some(ScriptEvent::Action(ScriptAction::MoveObjectBy {
                object_id: 7,
                dx: -4,
                dy: 2,
            }))
        );
        assert_eq!(
            runtime.advance(),
            Some(ScriptEvent::Action(ScriptAction::SetObjectLayer {
                object_id: 12,
                layer: -10,
            }))
        );
        assert!(matches!(
            runtime.advance(),
            Some(ScriptEvent::Action(ScriptAction::WalkObjectTo {
                speed: 8,
                repeat_entry: 7,
                ..
            }))
        ));
    }

    #[test]
    fn cash_action_can_redirect_active_execution() {
        let mut runtime = ScriptRuntime::new(table(&[
            [0, 0, 0, 0],
            [0x001e, 0xfff6, 4, 0],
            [0xffff, 10, 0, 0],
            [0, 0, 0, 0],
            [0xffff, 20, 0, 0],
            [0, 0, 0, 0],
        ]));
        runtime.start(trigger(1));
        assert_eq!(
            runtime.advance(),
            Some(ScriptEvent::Action(ScriptAction::AdjustCash {
                amount: -10,
                insufficient_entry: 4,
            }))
        );
        assert!(runtime.branch_to(4));
        assert_eq!(
            runtime.advance(),
            Some(ScriptEvent::Message {
                message_id: 20,
                position: DialogPosition::Lower,
                font_color: 0x4f,
                face_index: None,
            })
        );
    }

    #[test]
    fn yields_persistent_state_conditions_and_batch_mutation() {
        let mut runtime = ScriptRuntime::new(table(&[
            [0, 0, 0, 0],
            [0x0058, 9, 2, 7],
            [0x0094, 0xffff, 0xffff, 8],
            [0x0095, 3, 9, 0],
            [0x009a, 4, 6, 0xfffe],
            [0, 0, 0, 0],
        ]));
        runtime.start(trigger(1));
        assert_eq!(
            runtime.advance(),
            Some(ScriptEvent::Condition(ScriptCondition::ItemCountLess {
                item_id: 9,
                amount: 2,
                target_entry: 7,
            }))
        );
        assert_eq!(
            runtime.advance(),
            Some(ScriptEvent::Condition(ScriptCondition::ObjectStateEquals {
                object_id: 7,
                state: -1,
                target_entry: 8,
            }))
        );
        assert_eq!(
            runtime.advance(),
            Some(ScriptEvent::Condition(ScriptCondition::SceneEquals {
                scene_number: 3,
                target_entry: 9,
            }))
        );
        assert_eq!(
            runtime.advance(),
            Some(ScriptEvent::Action(ScriptAction::SetObjectStates {
                first_object_id: 4,
                last_object_id: 6,
                state: -2,
            }))
        );
    }
}
