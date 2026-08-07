//! 原版 PAL 脚本 opcode 的权威目录和元数据。

/// 负责处理 opcode 的触发脚本 handler。
///
/// 它和 opcode 目录定义在一起，确保新增 opcode 时不会遗漏触发脚本的分发关系。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TriggerHandler {
    Control,
    Presentation,
    Scene,
    Role,
    Battle,
    Condition,
}

macro_rules! define_script_opcodes {
    ($(
        $(#[$meta:meta])*
        $variant:ident = $value:literal, $handler:ident;
    )+) => {
        /// 原版 PAL 脚本引擎处理的完整 opcode 集合。
        ///
        /// 原始指令集中的 `0x0032`、`0x0048`、`0x0072` 和 `0x009D` 是空洞，
        /// 因此不包含在枚举中；`0xFFFF` 是消息伪指令。
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        #[repr(u16)]
        pub enum ScriptOpcode {
            $(
                $(#[$meta])*
                $variant = $value,
            )+
        }

        impl ScriptOpcode {
            /// 原版解释器中的全部 opcode，按数值排列。
            pub const ALL: &'static [Self] = &[$(Self::$variant,)+];

            pub const fn raw(self) -> u16 {
                self as u16
            }

            /// 供诊断和工具使用的 Rust 变体名。
            pub const fn name(self) -> &'static str {
                match self {
                    $(Self::$variant => stringify!($variant),)+
                }
            }

            pub(super) const fn trigger_handler(self) -> TriggerHandler {
                match self {
                    $(Self::$variant => TriggerHandler::$handler,)+
                }
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
                formatter.write_str(self.name())
            }
        }
    };
}

define_script_opcodes! {
    /// 停止当前脚本。
    Stop = 0x0000, Control;
    /// 停止脚本，并将下一条指令持久化为新入口。
    StopAndAdvance = 0x0001, Control;
    /// 停止脚本，并将操作数 0 指定的入口持久化为新入口。
    StopAndReplace = 0x0002, Control;
    /// 无条件跳转到操作数 0 指定的入口。
    Jump = 0x0003, Control;
    /// 调用操作数 0 指定的脚本；操作数 1 可指定另一个所有者。
    Call = 0x0004, Control;
    /// 重绘屏幕并按指定时长延迟。
    Redraw = 0x0005, Presentation;
    /// 根据操作数 0 指定的概率跳转。
    JumpByChance = 0x0006, Control;
    /// 开始战斗并根据战斗结果分支。
    StartBattle = 0x0007, Battle;
    /// 将下一条指令持久化为脚本入口并继续执行。
    AdvanceEntry = 0x0008, Control;
    /// 等待操作数 0 个场景帧。
    WaitFrames = 0x0009, Control;
    /// 显示确认菜单；选择“否”时跳转到操作数 0。
    Confirm = 0x000A, Presentation;
    /// 让当前事件对象向南走一步。
    WalkObjectSouth = 0x000B, Scene;
    /// 让当前事件对象向西走一步。
    WalkObjectWest = 0x000C, Scene;
    /// 让当前事件对象向北走一步。
    WalkObjectNorth = 0x000D, Scene;
    /// 让当前事件对象向东走一步。
    WalkObjectEast = 0x000E, Scene;
    /// 设置当前事件对象的方向和／或动画帧。
    SetObjectPose = 0x000F, Scene;
    /// 让当前事件对象以普通脚本速度走到指定地图格。
    WalkObjectTo = 0x0010, Scene;
    /// 让当前事件对象以较慢速度走到指定地图格。
    WalkObjectToSlow = 0x0011, Scene;
    /// 相对队伍位置设置一个事件对象的位置。
    SetObjectPositionRelative = 0x0012, Scene;
    /// 设置一个事件对象在场景中的绝对位置。
    SetObjectPosition = 0x0013, Scene;
    /// 让当前事件对象朝南并设置动作帧。
    SetObjectGesture = 0x0014, Scene;
    /// 设置一名队员的方向和动作帧。
    SetPartyMemberPose = 0x0015, Scene;
    /// 设置指定事件对象的方向和动作帧。
    SetSelectedObjectPose = 0x0016, Scene;
    /// 设置一项由装备产生的角色额外属性。
    SetEquipmentEffect = 0x0017, Role;
    /// 让脚本所有者角色装备选中的物品。
    EquipItem = 0x0018, Role;
    /// 增加或减少一项角色属性。
    AdjustPlayerAttribute = 0x0019, Role;
    /// 将一项角色属性设为绝对值。
    SetPlayerAttribute = 0x001A, Role;
    /// 增加或减少一名角色或全队的 HP。
    AdjustPlayerHp = 0x001B, Role;
    /// 增加或减少一名角色或全队的 MP。
    AdjustPlayerMp = 0x001C, Role;
    /// 以相同数值增加或减少 HP 和 MP。
    AdjustPlayerHpMp = 0x001D, Role;
    /// 增加或减少队伍金钱；余额不足时分支。
    AdjustCash = 0x001E, Role;
    /// 向背包添加物品。
    AddItem = 0x001F, Role;
    /// 从背包或队员装备中移除物品。
    RemoveItem = 0x0020, Role;
    /// 对一个或全部敌人造成直接伤害。
    DamageEnemy = 0x0021, Battle;
    /// 复活一名角色或全队倒下的角色。
    RevivePlayer = 0x0022, Role;
    /// 移除角色的一个或全部装备槽。
    RemoveEquipment = 0x0023, Role;
    /// 设置事件对象的自动脚本入口。
    SetObjectAutoScript = 0x0024, Scene;
    /// 设置事件对象的触发脚本入口。
    SetObjectTriggerScript = 0x0025, Scene;
    /// 打开指定商店的购买菜单。
    OpenBuyMenu = 0x0026, Presentation;
    /// 打开背包出售菜单。
    OpenSellMenu = 0x0027, Presentation;
    /// 对敌人施加指定毒对象。
    PoisonEnemy = 0x0028, Battle;
    /// 对角色施加指定毒对象。
    PoisonPlayer = 0x0029, Battle;
    /// 移除敌人的指定毒对象。
    CureEnemyPoison = 0x002A, Battle;
    /// 移除角色的指定毒对象。
    CurePlayerPoison = 0x002B, Battle;
    /// 移除角色身上不高于指定等级的中毒。
    CurePoisonByLevel = 0x002C, Battle;
    /// 对角色施加临时状态。
    SetPlayerStatus = 0x002D, Battle;
    /// 对敌人施加临时状态。
    SetEnemyStatus = 0x002E, Battle;
    /// 移除角色的临时状态。
    RemovePlayerStatus = 0x002F, Battle;
    /// 按基础值百分比临时替换角色的一项额外属性效果。
    AdjustTemporaryPlayerStat = 0x0030, Battle;
    /// 临时替换角色的战斗精灵。
    SetTemporaryBattleSprite = 0x0031, Battle;
    /// 收取敌人以便之后转化为物品。
    CollectEnemy = 0x0033, Battle;
    /// 将已收取的敌人转化为物品。
    TransmuteCollectedEnemies = 0x0034, Battle;
    /// 按指定时长和强度震动屏幕。
    ShakeScreen = 0x0035, Presentation;
    /// 选择当前 RNG 动画资源。
    SelectRngAnimation = 0x0036, Presentation;
    /// 播放已选 RNG 动画的指定帧。
    PlayRngAnimation = 0x0037, Presentation;
    /// 执行当前场景的传送脚本；失败时分支。
    TeleportParty = 0x0038, Scene;
    /// 从敌人吸取 HP 并转给当前行动角色。
    DrainEnemyHp = 0x0039, Battle;
    /// 尝试逃离战斗。
    FleeBattle = 0x003A, Battle;
    /// 让后续对话显示在屏幕中央。
    DialogCenter = 0x003B, Presentation;
    /// 让后续对话显示在屏幕上方。
    DialogUpper = 0x003C, Presentation;
    /// 让后续对话显示在屏幕下方。
    DialogLower = 0x003D, Presentation;
    /// 让后续文本显示在居中窗口中。
    DialogCenterWindow = 0x003E, Presentation;
    /// 让队伍乘坐当前事件对象低速移动到指定地图格。
    RideObjectSlow = 0x003F, Scene;
    /// 设置事件对象的交互触发方式。
    SetObjectTriggerMode = 0x0040, Scene;
    /// 将当前脚本执行标记为失败。
    MarkScriptFailed = 0x0041, Control;
    /// 在战斗中模拟一次角色法术攻击。
    SimulatePlayerMagic = 0x0042, Battle;
    /// 播放或停止场景背景音乐。
    PlayMusic = 0x0043, Presentation;
    /// 让队伍乘坐当前事件对象以普通速度移动到指定地图格。
    RideObject = 0x0044, Scene;
    /// 设置下一场战斗的音乐编号。
    SetBattleMusic = 0x0045, Battle;
    /// 设置队伍在当前地图中的位置。
    SetPartyPosition = 0x0046, Scene;
    /// 播放音效。
    PlaySound = 0x0047, Presentation;
    /// 设置事件对象状态。
    SetObjectState = 0x0049, Scene;
    /// 设置下一场战斗的战场编号。
    SetBattlefield = 0x004A, Battle;
    /// 短暂隐藏当前事件对象。
    HideObjectShort = 0x004B, Scene;
    /// 让当前事件对象追逐玩家。
    ChasePlayer = 0x004C, Scene;
    /// 等待玩家按键。
    WaitForKey = 0x004D, Presentation;
    /// 读取最近一次存档。
    LoadLastSave = 0x004E, Presentation;
    /// 将屏幕淡变为红色以显示游戏结束。
    FadeToRed = 0x004F, Presentation;
    /// 淡出屏幕。
    FadeOut = 0x0050, Presentation;
    /// 淡入屏幕。
    FadeIn = 0x0051, Presentation;
    /// 按指定时长隐藏当前事件对象。
    HideObject = 0x0052, Scene;
    /// 切换到日间调色板。
    UseDayPalette = 0x0053, Presentation;
    /// 切换到夜间调色板。
    UseNightPalette = 0x0054, Presentation;
    /// 让角色习得指定法术。
    AddMagic = 0x0055, Role;
    /// 移除角色的指定法术。
    RemoveMagic = 0x0056, Role;
    /// 根据消耗的 MP 设置法术基础伤害。
    ScaleMagicByMp = 0x0057, Battle;
    /// 持有物品少于指定数量时跳转。
    JumpIfItemCountLess = 0x0058, Condition;
    /// 切换到指定场景。
    ChangeScene = 0x0059, Scene;
    /// 将角色 HP 减半。
    HalvePlayerHp = 0x005A, Battle;
    /// 将敌人 HP 减半。
    HalveEnemyHp = 0x005B, Battle;
    /// 在指定时间内隐藏战斗角色。
    HideBattleActor = 0x005C, Battle;
    /// 角色没有指定中毒时跳转。
    JumpIfPlayerLacksPoison = 0x005D, Condition;
    /// 敌人没有指定中毒时跳转。
    JumpIfEnemyLacksPoison = 0x005E, Condition;
    /// 立即击倒角色。
    KillPlayer = 0x005F, Battle;
    /// 立即击倒敌人。
    KillEnemy = 0x0060, Battle;
    /// 角色没有任何中毒时跳转。
    JumpIfPlayerNotPoisoned = 0x0061, Condition;
    /// 在指定时间内暂停敌人追逐。
    PauseEnemyChase = 0x0062, Scene;
    /// 在指定时间内加快敌人追逐。
    SpeedUpEnemyChase = 0x0063, Scene;
    /// 敌人 HP 高于指定百分比时跳转。
    JumpIfEnemyHpAbove = 0x0064, Condition;
    /// 设置角色的场景精灵，并可重新加载当前队伍精灵。
    SetPlayerSprite = 0x0065, Role;
    /// 对敌人模拟一次投掷武器法术攻击。
    ThrowWeapon = 0x0066, Battle;
    /// 设置敌人使用的法术及施法概率。
    EnemyCastMagic = 0x0067, Battle;
    /// 当前处于敌方行动时跳转。
    JumpIfEnemyTurn = 0x0068, Condition;
    /// 让敌方队伍逃跑并结束战斗。
    EnemyEscape = 0x0069, Battle;
    /// 从敌人身上偷取物品或金钱。
    StealEnemy = 0x006A, Battle;
    /// 对敌人施加战场位移。
    BlowEnemiesAway = 0x006B, Battle;
    /// 偏移事件对象并推进其动画。
    OffsetObjectAndAnimate = 0x006C, Scene;
    /// 设置场景的进入脚本和传送脚本入口。
    SetSceneScripts = 0x006D, Scene;
    /// 按有符号像素偏移移动队伍。
    OffsetParty = 0x006E, Scene;
    /// 从另一个匹配的事件对象复制状态。
    SyncObjectState = 0x006F, Scene;
    /// 让队伍以普通脚本速度走到指定地图格。
    WalkParty = 0x0070, Scene;
    /// 设置屏幕波动效果。
    SetScreenWave = 0x0071, Presentation;
    /// 从备份画面淡变到当前场景。
    FadeScene = 0x0073, Presentation;
    /// 任一队员 HP 未满时跳转。
    JumpIfPartyNotFullHp = 0x0074, Condition;
    /// 替换当前队伍成员。
    SetParty = 0x0075, Role;
    /// 显示一张 FBP 全屏图片。
    ShowFbp = 0x0076, Presentation;
    /// 停止当前音乐，并可选择淡出。
    StopMusic = 0x0077, Presentation;
    /// 原版保留的兼容空操作，历史用途不明。
    NoOp = 0x0078, Control;
    /// 队伍中存在指定角色名时跳转。
    JumpIfPartyContainsPlayer = 0x0079, Condition;
    /// 让队伍快速走到指定地图格。
    WalkPartyFast = 0x007A, Scene;
    /// 让队伍以最高速度走到指定地图格。
    WalkPartyFastest = 0x007B, Scene;
    /// 让当前事件对象每隔一个原版帧走向指定地图格。
    WalkObjectHalfSpeed = 0x007C, Scene;
    /// 按有符号像素偏移移动事件对象。
    OffsetObject = 0x007D, Scene;
    /// 设置事件对象的绘制层级。
    SetObjectLayer = 0x007E, Scene;
    /// 移动、锁定或恢复视口。
    MoveViewport = 0x007F, Scene;
    /// 在日间和夜间调色板之间切换。
    ToggleDayNightPalette = 0x0080, Presentation;
    /// 玩家没有面向指定事件对象时跳转。
    JumpIfNotFacingObject = 0x0081, Condition;
    /// 让当前事件对象快速走到指定地图格。
    WalkObjectFast = 0x0082, Scene;
    /// 事件对象位于另一个对象的范围外时跳转。
    JumpIfObjectOutsideZone = 0x0083, Condition;
    /// 将当前使用的物品作为事件对象放入场景。
    PlaceUsedItemObject = 0x0084, Scene;
    /// 延迟操作数 0 个 80 毫秒周期。
    Delay = 0x0085, Control;
    /// 已装备的指定物品少于要求数量时跳转。
    JumpIfItemNotEquipped = 0x0086, Condition;
    /// 推进事件对象动画一帧。
    AnimateObject = 0x0087, Scene;
    /// 消耗金钱并据此计算法术基础伤害。
    ScaleMagicByCash = 0x0088, Battle;
    /// 设置当前战斗结果。
    SetBattleResult = 0x0089, Battle;
    /// 为下一场战斗启用自动选招。
    EnableAutoBattle = 0x008A, Battle;
    /// 切换当前调色板编号。
    SetPalette = 0x008B, Presentation;
    /// 从指定调色板颜色淡入或淡出。
    FadeColor = 0x008C, Presentation;
    /// 提升角色等级。
    LevelUpPlayer = 0x008D, Role;
    /// 恢复此前备份的屏幕。
    RestoreScreen = 0x008E, Presentation;
    /// 将队伍金钱减半。
    HalveCash = 0x008F, Role;
    /// 替换全局对象定义中的一个脚本字段。
    SetObjectScript = 0x0090, Scene;
    /// 敌人不是同类中的首个存活实例时跳转。
    JumpIfEnemyNotFirstKind = 0x0091, Condition;
    /// 播放角色的战斗施法动画。
    PlayerMagicAnimation = 0x0092, Battle;
    /// 重建场景的同时执行画面淡变。
    FadeSceneWithUpdate = 0x0093, Presentation;
    /// 事件对象状态等于操作数 1 时跳转。
    JumpIfObjectStateEquals = 0x0094, Condition;
    /// 当前场景等于操作数 0 时跳转。
    JumpIfSceneEquals = 0x0095, Condition;
    /// 播放 DOS 版结局动画。
    PlayEndingAnimation = 0x0096, Presentation;
    /// 让队伍乘坐当前事件对象高速移动到指定地图格。
    RideObjectFast = 0x0097, Scene;
    /// 设置或清除队伍跟随角色。
    SetPartyFollower = 0x0098, Role;
    /// 修改场景使用的地图编号。
    SetSceneMap = 0x0099, Scene;
    /// 为连续范围内的事件对象批量设置同一状态。
    SetObjectStates = 0x009A, Scene;
    /// 按原版兼容行为淡变到当前场景。
    FadeToCurrentScene = 0x009B, Presentation;
    /// 将一个敌人分裂为更多副本。
    DivideEnemy = 0x009C, Battle;
    /// 让敌人召唤另一个怪物。
    SummonEnemy = 0x009E, Battle;
    /// 将敌人变换为另一个对象。
    TransformEnemy = 0x009F, Battle;
    /// 执行结局流程并退出游戏。
    QuitGame = 0x00A0, Presentation;
    /// 将所有队员和轨迹点收拢到队长位置。
    CollapseParty = 0x00A1, Role;
    /// 从后续操作数 0 条指令中随机选择一条执行。
    RandomSelect = 0x00A2, Control;
    /// 播放 CD 音轨，并以普通音乐作为回退。
    PlayCdMusic = 0x00A3, Presentation;
    /// 将 FBP 图片滚动显示到屏幕上。
    ScrollFbp = 0x00A4, Presentation;
    /// 显示 FBP 图片并叠加结局精灵效果。
    ShowFbpWithSprite = 0x00A5, Presentation;
    /// 备份当前屏幕以供后续转场使用。
    BackupScreen = 0x00A6, Presentation;
    /// 推进自动脚本但不产生任何效果。
    AutoScriptNoOp = 0x00A7, Control;
    /// 显示操作数 0 指定的消息。
    PrintMessage = 0xFFFF, Presentation;
}
