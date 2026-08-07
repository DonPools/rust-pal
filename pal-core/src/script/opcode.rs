//! Canonical catalog and metadata for original PAL script opcodes.

/// Whole-engine coverage for an original PAL script opcode.
///
/// This combines trigger and automatic-script execution. An instruction such
/// as [`ScriptOpcode::ChasePlayer`] can therefore be implemented overall while
/// remaining invalid in a trigger script.
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
    BlowEnemiesAway = 0x006B, "BLOW_ENEMIES", "Apply a battlefield displacement to enemies.", Implemented;
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
    SetObjectScript = 0x0090, "OBJECT_SCRIPT", "Replace one script field in a global object definition.", Implemented;
    JumpIfEnemyNotFirstKind = 0x0091, "JNOT_FIRST_ENEMY", "Jump when an enemy is not the first living instance of its kind.", Implemented;
    PlayerMagicAnimation = 0x0092, "MAGIC_ANIM", "Show a player's battle magic-casting animation.", Implemented;
    FadeSceneWithUpdate = 0x0093, "SCENE_FADE_UPDATE", "Fade the screen while rebuilding the scene.", Implemented;
    JumpIfObjectStateEquals = 0x0094, "JEQ_OBJ_STATE", "Jump when an event object's state equals operand 1.", Implemented;
    JumpIfSceneEquals = 0x0095, "JEQ_SCENE", "Jump when the current scene equals operand 0.", Implemented;
    PlayEndingAnimation = 0x0096, "ENDING", "Play the DOS ending animation.", Implemented;
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
