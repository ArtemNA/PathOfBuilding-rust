//! The skill functions of `Data/Skills/*.lua` (R7-ARCH §1 p. 3): every closure `data/closures.json` records under `Data/Skills/` (134),
//! keyed by `<src>:<from>-<to>` with the sha256 of its body (LF-normalised lines `from..to`), as the ModParser/config registries are
//! -- drift test `tests/offence_cp.rs::skill_func_registry_matches_closures_json` over [`skill_func_entries`].
//!
//! Each entry is one of:
//! * [`Body::Port`] -- a hand port in [`funcs`] (several skills share a body; the sha says which);
//! * [`Body::Unported`] -- an explicit gap: calling it records `"<src>:<from>-<to> <path>: <why>"` in `env.unimplemented`;
//!
//! `runSkillFunc` (CalcOffence.lua:509-514) looks the active effect's hook up here as `skills.<grantedEffect.id>.<hook>`.

mod funcs;

use crate::offence::Off;
/// The offence checkpoint view of `output.MainHand` / `output.OffHand` (the offence module is crate-private; `lib.rs` is outside the
/// R7.4 card's files, so the probe accessor is re-exported here).
pub use crate::offence::{HandTable, checkpoint_hand_outputs};
use funcs as f;

/// What a registry entry does when called.
#[derive(Clone, Copy)]
pub(crate) enum Body {
    Port(fn(&mut Off<'_>)),
    Unported(&'static str),
}

/// One `data/closures.json` skill closure.
#[derive(Clone, Copy)]
pub(crate) struct SkillFuncEntry {
    /// `skills.<id>.<hook>` (or `skills.<id>.statMap._grantedEffect.<hook>`).
    pub(crate) path: &'static str,
    /// The file under `src/Data/Skills/`.
    pub(crate) src: &'static str,
    pub(crate) from: u32,
    pub(crate) to: u32,
    pub(crate) sha: &'static str,
    pub(crate) body: Body,
}

const fn e(path: &'static str, src: &'static str, from: u32, to: u32, sha: &'static str, body: Body) -> SkillFuncEntry {
    SkillFuncEntry {
        path,
        src,
        from,
        to,
        sha,
        body,
    }
}

use Body::{Port, Unported};

/// Every skill closure of `data/closures.json`, in its order.
#[rustfmt::skip]
pub(crate) static SKILL_FUNCS: [SkillFuncEntry; 134] = [
    e("skills.ArmageddonBrand.preDamageFunc", "act_int.lua", 715, 717, "48907eac3b6310cb369ef87ebddefa1a756bd1661da336193b0eaae2df3d17c6", Port(f::brand_activation)),
    e("skills.ArmageddonBrandAltY.preDamageFunc", "act_int.lua", 895, 897, "48907eac3b6310cb369ef87ebddefa1a756bd1661da336193b0eaae2df3d17c6", Port(f::brand_activation)),
    e("skills.AvengingFlame.preDamageFunc", "sup_str.lua", 2540, 2548, "faaad88be39c14af61f127e760f1f689f9d4aae599fe769597b20a36b665670a", Unported("GlobalCache.cachedData MAIN/CALCS of the trigger source (Env.player.output.TotemLife; CALCS mode not ported)")),
    e("skills.AzmeriHydraBarrage.preDamageFunc", "spectre.lua", 7277, 7279, "03e59e6faf7109e4a7a2a36ece5042d3240adfa86c9ca60ff39b899a3d699a99", Port(f::dps_eq_projectile_count)),
    e("skills.BallLightning.preDamageFunc", "act_int.lua", 1086, 1167, "dcb69ca3835a1191fe7c523895792508563f293fe432f147c747c60c6fef139e", Port(f::ball_lightning)),
    e("skills.BallLightningAltX.preDamageFunc", "act_int.lua", 1252, 1275, "89c427cc7e2f34a3c66e83beae5543c68e59253f28cdcf5b0553160143440188", Port(f::ball_lightning_alt_x)),
    e("skills.BallLightningAltY.preDamageFunc", "act_int.lua", 1375, 1393, "cba9cc4100f856123d3325e56396627834bba054a40ccf8b836925fe7c4adccf", Port(f::ball_lightning_alt_y)),
    e("skills.Bane.preSkillTypeFunc", "act_int.lua", 1470, 1481, "fba3ceee7fea2793a2d314ed507e81d59d47f9c9e2b09dcbeb6e02863adfaca2", Port(f::bane)),
    e("skills.BaneAltX.preSkillTypeFunc", "act_int.lua", 1647, 1658, "fba3ceee7fea2793a2d314ed507e81d59d47f9c9e2b09dcbeb6e02863adfaca2", Port(f::bane)),
    e("skills.Barrage.preDamageFunc", "act_dex.lua", 1026, 1030, "659342bba434fa868ca4478a8ef9e416bb275f9da00e180d681b210af698ca1b", Port(f::part2_dps_eq_projectile_count)),
    e("skills.BarrageAltX.preDamageFunc", "act_dex.lua", 1114, 1118, "659342bba434fa868ca4478a8ef9e416bb275f9da00e180d681b210af698ca1b", Port(f::part2_dps_eq_projectile_count)),
    e("skills.BladeBlast.preDamageFunc", "act_dex.lua", 1374, 1379, "5b44bc750e2f5bb739428f164abf9c3a1177a4a0b152d5521a449d49dd43762d", Port(f::blade_blast)),
    e("skills.BladeBlastAltX.preDamageFunc", "act_dex.lua", 1466, 1471, "5b44bc750e2f5bb739428f164abf9c3a1177a4a0b152d5521a449d49dd43762d", Port(f::blade_blast)),
    e("skills.BladeFlurry.preDamageFunc", "act_dex.lua", 1906, 1916, "6fd38a79cb34281f091e02470a78846b6094afd6c97b288d5b65ebcc48ac918a", Port(f::blade_flurry)),
    e("skills.BladeFlurryAltX.preDamageFunc", "act_dex.lua", 2023, 2033, "6fd38a79cb34281f091e02470a78846b6094afd6c97b288d5b65ebcc48ac918a", Port(f::blade_flurry)),
    e("skills.BladeVortex.preDamageFunc", "act_dex.lua", 2209, 2211, "6d53d97ad73356d74c3ea60e46f26bfa2f76739c0ba480bae189da0fa2e82ba2", Port(f::blade_vortex)),
    e("skills.BladefallAltZ.preDamageFunc", "act_dex.lua", 2787, 2789, "52cad43550ec10e9b9a2108ca0e71eb5c44f50096eb28273e8cb5a3f9d208893", Port(f::bladefall_alt_z)),
    e("skills.BlastRain.preDamageFunc", "act_dex.lua", 2893, 2897, "659342bba434fa868ca4478a8ef9e416bb275f9da00e180d681b210af698ca1b", Port(f::part2_dps_eq_projectile_count)),
    e("skills.BlazingSalvo.preDamageFunc", "act_int.lua", 1823, 1827, "320418560c45149dd3f3e44b313abef1f0ce7ff4911d65edd1e021ac929f8d7d", Port(f::blazing_salvo)),
    e("skills.BloodSacramentUnique.initialFunc", "other.lua", 669, 677, "f8f41b09ba743ff19d8b31e7eb52f055e786e2ceddf542d5883aa858d865c970", Port(f::blood_sacrament)),
    e("skills.Bodyswap.preDamageFunc", "act_int.lua", 2298, 2309, "400f194111cbbdd3fa5526b6026439fa4da0643ee4126c8ea0c343b2bcdb7cc4", Port(f::bodyswap)),
    e("skills.BodyswapAltX.preDamageFunc", "act_int.lua", 2413, 2424, "400f194111cbbdd3fa5526b6026439fa4da0643ee4126c8ea0c343b2bcdb7cc4", Port(f::bodyswap)),
    e("skills.BrandSupport.preDamageFunc", "act_int.lua", 552, 554, "48907eac3b6310cb369ef87ebddefa1a756bd1661da336193b0eaae2df3d17c6", Port(f::brand_activation)),
    e("skills.ChargedDash.preDamageFunc", "act_dex.lua", 3939, 3944, "cbca4be3fb48ac1298af025e78cbe71d9b1da8b29161ee6f4d756bee523e1cf3", Port(f::charged_dash)),
    e("skills.ChargedDashAltX.preDamageFunc", "act_dex.lua", 4064, 4069, "4cfa3dd9c3ae1b1b8d266348465c2fbf8ccabc90fca8b3ad8d339b607a1af06e", Port(f::charged_dash_alt_x)),
    e("skills.Cremation.preDamageFunc", "act_dex.lua", 4355, 4359, "882fada5a9bfe6cb9fea48dbebacee399971e50d9abb7b1018570a1689744a52", Port(f::cremation_part1)),
    e("skills.CremationAltX.preDamageFunc", "act_dex.lua", 4473, 4477, "882fada5a9bfe6cb9fea48dbebacee399971e50d9abb7b1018570a1689744a52", Port(f::cremation_part1)),
    e("skills.CremationAltY.preDamageFunc", "act_dex.lua", 4592, 4594, "bd9b0643b619e82acac920480884590d96cb2cdbb4df6e037e96d72f5e6bccfc", Port(f::cremation)),
    e("skills.Cyclone.initialFunc", "act_dex.lua", 4703, 4716, "a17ea50f272107e6da0976d59b0338a9c02946a975073f5d1bf3dac7ee02727f", Port(f::cyclone)),
    e("skills.CycloneAltX.initialFunc", "act_dex.lua", 4809, 4822, "41ca5d9b2ee830be0c3c078a5e0c456f366600e6cb5dcce235ab941836797fd3", Port(f::cyclone_alt_x)),
    e("skills.DarkPact.preDamageFunc", "act_int.lua", 3889, 3903, "54cbc3a2f6744bc90aa534629f69e10239109e56e9631bea72eda678c3db98f5", Port(f::dark_pact)),
    e("skills.DarkPactAltX.preDamageFunc", "act_int.lua", 4006, 4022, "74942ee31d9fdf61da9a7d9ed8d6e73c1005d4b108bd08775d0363449bf0ec60", Port(f::dark_pact_alt_x)),
    e("skills.DeathWish.preDamageFunc", "other.lua", 1282, 1288, "30eefc9e444f08bc31fd1c9f01b822f3d1824a5d1c08de98d748550188dd96ba", Port(f::death_wish)),
    e("skills.DivineIre.preDamageFunc", "act_int.lua", 4602, 4606, "f9c1b539df20a924f9fd319a7addab2918186e24162e68189e7b42de466ae7f8", Port(f::divine_ire)),
    e("skills.DivineIreAltX.preDamageFunc", "act_int.lua", 4712, 4716, "25a809cb4fe5f0739cf8d3284c70e8dbdc2a3eef31f1ae187eab371f088f2ce8", Port(f::divine_ire_alt_x)),
    e("skills.DivineIreAltY.preDamageFunc", "act_int.lua", 4823, 4825, "7a4ba81a52f2a63bb6f18cda7ea66454dd7fb1ae2bc33adc50260434d39485f5", Port(f::divine_ire_alt_y)),
    e("skills.Earthquake.preDamageFunc", "act_str.lua", 3363, 3366, "f83e1ae0bdaf20278a8f98f4c547f3f91a4fbe9bd6074350753a14d1bf68d7fd", Port(f::earthquake)),
    e("skills.EnemyExplode.preDamageFunc", "other.lua", 6204, 6252, "7e9905c62fd2940c2fa3a44316dc64e87f0c8502f602d0c16b061cbb04e58913", Port(f::enemy_explode)),
    e("skills.ExplosiveArrow.explosiveArrowFunc", "act_dex.lua", 6699, 6778, "fcb10aac6ef439a9aeda38d6950f55f88ee90e682f5888b661c270c448f78105", Port(f::explosive_arrow)),
    e("skills.ExplosiveTrap.preDamageFunc", "act_dex.lua", 7156, 7221, "d3c4aeaf80b651612cead32b56b5483cc01d01b91446d8cd1fb324ead204a510", Unported("pairs(output.AreaOfEffectRadiusTertiaryOccurrences): float sum in lj_tab order (R11)")),
    e("skills.ExplosiveTrap.statMap._grantedEffect.preDamageFunc", "act_dex.lua", 7156, 7221, "d3c4aeaf80b651612cead32b56b5483cc01d01b91446d8cd1fb324ead204a510", Unported("pairs(output.AreaOfEffectRadiusTertiaryOccurrences): float sum in lj_tab order (R11)")),
    e("skills.EyeOfWinter.preDamageFunc", "act_int.lua", 5518, 5520, "0a1071f37209aa0bde8bc91a7ab1bb25d949adf9554f2fc4afb04d0d4e96aa5f", Port(f::eye_of_winter)),
    e("skills.EyeOfWinterAltX.preDamageFunc", "act_int.lua", 5611, 5613, "505fe42709ddbbd02464f56c05a1886d33b00377fb63f80c40d793817ff14d9d", Port(f::eye_of_winter_alt_x)),
    e("skills.EyeOfWinterAltY.preDamageFunc", "act_int.lua", 5704, 5706, "cde5535b049367c04282e6d0f8c0ff11409e8ec2ea4f6a93691d0875a0fb29a7", Port(f::eye_of_winter_alt_y)),
    e("skills.Flameblast.preDamageFunc", "act_int.lua", 6800, 6802, "d91f3f655684a8ae1337eac08c1fc1191699d5fc5e89d66303764d6fe9b31e36", Port(f::flameblast)),
    e("skills.FlameblastAltX.preDamageFunc", "act_int.lua", 6905, 6907, "f96210fe53a013ffa40a757544f282747f78617d8da9a269ffc278ee9ee89556", Port(f::flameblast_alt_x)),
    e("skills.FlameblastAltY.preDamageFunc", "act_int.lua", 7008, 7010, "251dfcbb9351c5830cd56f1e801ead6f33c4e6cf877fd6ff6a074bf6cabc2117", Port(f::flameblast_alt_y)),
    e("skills.FlamethrowerTrap.preDamageFunc", "act_dex.lua", 7691, 7717, "6cdc3e1700dfe5b0913fd558ea38bb7f67196eb5f76e944bbd6acc5b667d854e", Port(f::flamethrower_trap)),
    e("skills.FlamethrowerTrapAltX.preDamageFunc", "act_dex.lua", 7809, 7827, "5fba2e220c7694bd1b0e58ea4828866dbe8953c59861746ec69d13a68e266d95", Port(f::flamethrower_trap_alt_x)),
    e("skills.ForbiddenRite.preDamageFunc", "act_int.lua", 7390, 7434, "8457f4596105fea7e4ec5004af033184441926d8a9011141ff1701f05563ba0d", Unported("TotemChaosResist / self damage taken (not ported)")),
    e("skills.ForbiddenRiteAltX.preDamageFunc", "act_int.lua", 7549, 7589, "b3b9441a39c2cafc17cd4c2c2b55e57b984501ce03215e2da96de822e277fee6", Unported("TotemChaosResist / self damage taken (not ported)")),
    e("skills.FreezingPulse.preDamageFunc", "act_int.lua", 7686, 7689, "06ba986fa54db9309760acd69ad078489a2c6dfe3714aa04413a79384595c94a", Port(f::freezing_pulse)),
    e("skills.FrostBombAltY.preDamageFunc", "act_int.lua", 7933, 7936, "2ca978412425995c8196876064185d0aa15abe800fd24b4d2e08ce4aa5b9e1fb", Port(f::frost_bomb_alt_y)),
    e("skills.FrozenSweep.preDamageFunc", "act_str.lua", 4679, 4713, "2723a80de969128512021a766ddcc18b794345df65a8d02b7f248081e64f7073", Unported("parent skill (Frozen Legion) lookup via actor.mainSkill.socketGroup.slot (not ported)")),
    e("skills.FrozenSweepAltX.preDamageFunc", "act_str.lua", 4797, 4831, "6e21ef23c19d262d7b7f44d7bd8cb3d6592b9f54e97155a45950adf17c31dc4d", Unported("parent skill (Frozen Legion of Rallying) lookup via actor.mainSkill.socketGroup.slot (not ported)")),
    e("skills.GAZombieCorpseGroundImpact.preDamageFunc", "minion.lua", 756, 758, "bc3a99f2a20c2aa3d459f84684ca97c7f18ea23622b4c852bfdd0a67f9443ecb", Unported("activeSkill.summonSkill (minion skills, R7.10)")),
    e("skills.GalvanicField.preDamageFunc", "act_int.lua", 8543, 8545, "c46c160cb7ed4e7493e4c23dcc87fb5820553396bc2c9f1589a0b1e5dd39eac6", Port(f::hit_time_repeat_frequency)),
    e("skills.GalvanicFieldAltX.preDamageFunc", "act_int.lua", 8663, 8665, "c46c160cb7ed4e7493e4c23dcc87fb5820553396bc2c9f1589a0b1e5dd39eac6", Port(f::hit_time_repeat_frequency)),
    e("skills.HeraldOfAsh.preDamageFunc", "act_str.lua", 5743, 5745, "b2dc72cf68544399285372853b14f33a4ea9ffd7a65c100bcb6c05e8251636d2", Port(f::herald_of_ash)),
    e("skills.HeraldOfTheBreach.preDamageFunc", "other.lua", 2258, 2260, "80f82d62eb8f1acd1ab5c0fc61896af3a6cb3d1531a582e287fd1982e7237541", Port(f::herald_of_the_breach)),
    e("skills.HeraldOfThunder.preDamageFunc", "act_int.lua", 9383, 9385, "3398c8cc4a2056f42ebb76d7a3099234dae24766f46b624cbb9208f2fe618343", Port(f::herald_of_thunder)),
    e("skills.Hydrosphere.preDamageFunc", "act_int.lua", 8972, 8976, "49c97908eb081a0b6d019e28727e7a2f3fc6451068ce7dfcbfd29fb38b31e919", Port(f::hydrosphere)),
    e("skills.IceSpear.preDamageFunc", "act_int.lua", 9805, 9809, "125b4c43fa4c1ca7013b5cae25905d7bd974e9f2b827a0b6e4c9bd1fce943279", Port(f::ice_spear)),
    e("skills.IceSpearAltX.preDamageFunc", "act_int.lua", 9915, 9919, "125b4c43fa4c1ca7013b5cae25905d7bd974e9f2b827a0b6e4c9bd1fce943279", Port(f::ice_spear)),
    e("skills.Incinerate.preDamageFunc", "act_int.lua", 10284, 10288, "d6cb072ea9bff8bb8feca577ce5f6bbd96fdcae3dbc5e27c8740f86aef289f11", Port(f::incinerate)),
    e("skills.IncinerateAltX.preDamageFunc", "act_int.lua", 10422, 10426, "12a8c701214603e8d50ce7f526488d83fd8c6ad500ff34e4c3d7fa4b8997d9e4", Port(f::incinerate_alt_x)),
    e("skills.IncinerateAltY.preDamageFunc", "act_int.lua", 10561, 10565, "b61aa681cbe1828858a0c35fdfad00007843ae8c0fac92c0902fb7b526bfd1dd", Port(f::incinerate_alt_y)),
    e("skills.InfernalBlow.preDamageFunc", "act_str.lua", 7115, 7123, "51d48b99032e2cafffa213dfd711a775ddfa0608c6f184c6aceb084e9c16c0d2", Port(f::infernal_blow)),
    e("skills.InfernalBlowAltX.preDamageFunc", "act_str.lua", 7241, 7249, "e7d573845e95ae52fe78cb4fbf8db7fffad8426a8b1326ae229343ccaf7dcff1", Port(f::infernal_blow_alt_x)),
    e("skills.KineticFusillade.postCritFunc", "act_int.lua", 11046, 11123, "84ce184bd3aec16164374c36bedcfed0b740bad90f48d0de247dfc004c135304", Port(f::kinetic_fusillade_post_crit)),
    e("skills.KineticFusillade.preDamageFunc", "act_int.lua", 11009, 11045, "7894a531ba5645f60951d837ce03583965a144c4ef3f22c9e8e69d26f816367a", Port(f::kinetic_fusillade)),
    e("skills.KineticFusilladeAltX.postCritFunc", "act_int.lua", 11264, 11341, "84ce184bd3aec16164374c36bedcfed0b740bad90f48d0de247dfc004c135304", Port(f::kinetic_fusillade_post_crit)),
    e("skills.KineticFusilladeAltX.preDamageFunc", "act_int.lua", 11227, 11263, "ff8dd4f936006f9cd573b6c8cac571d1bbbf4b3e206a59c1f7576c9e208b1e3c", Port(f::kinetic_fusillade_alt_x)),
    e("skills.LancingSteel.preDamageFunc", "act_dex.lua", 10234, 10241, "78ba4c2f5a3bc6abc7c60ced2ec7a1ef808108754657bbf0ace863c3a0cc87d2", Port(f::lancing_steel)),
    e("skills.LancingSteelAltX.preDamageFunc", "act_dex.lua", 10337, 10344, "224eb660840035750561b12ee3d7e1b4598b6cf3c992dd97dae3a67aee13f50b", Port(f::lancing_steel_alt_x)),
    e("skills.LightningBolt.preDamageFunc", "other.lua", 2596, 2598, "bcb877cd38f680213da072ab311e27cfa9cb87d1faa786617e844738aad9537e", Port(f::hit_time_cooldown)),
    e("skills.LightningSpireTrap.preDamageFunc", "act_int.lua", 11667, 11788, "d7337e59d5c21fa35d46d46e596f8b5884ac6223f66675756b9e367f0ebf1ea8", Port(f::lightning_spire_trap)),
    e("skills.LightningSpireTrapAltX.preDamageFunc", "act_int.lua", 11912, 12028, "028ba06841b97aabc71d46f9e1889238deb8252d56389e4d5b41adc296ab402f", Port(f::lightning_spire_trap_alt_x)),
    e("skills.LightningSpireTrapAltY.preDamageFunc", "act_int.lua", 12150, 12260, "ef24fbb9e65b31ff10dda30110edff92b135ceffd54395d20f91c0e5198ffaa1", Port(f::lightning_spire_trap_alt_y)),
    e("skills.LightningTendrilsAltX.postCritFunc", "act_int.lua", 12651, 12662, "1d9effc02375c0875ba97ed557181296c4fb298a2994b5923f902c3162a2c0f5", Port(f::lightning_tendrils_alt_x_post_crit)),
    e("skills.LightningTendrilsAltX.preDamageFunc", "act_int.lua", 12642, 12650, "b646128c0d7652d7854f4684bbe6be3c545a3739c7803643487b494bb9be0dad", Port(f::lightning_tendrils_alt_x)),
    e("skills.Manabond.preDamageFunc", "act_int.lua", 13527, 13532, "bb99045da4e3a30496e00a37107e19a7760dc43a6c9020f4c6e1ca2909836783", Port(f::manabond)),
    e("skills.MinionInstability.preDamageFunc", "minion.lua", 1756, 1758, "bc3a99f2a20c2aa3d459f84684ca97c7f18ea23622b4c852bfdd0a67f9443ecb", Unported("activeSkill.summonSkill (minion skills, R7.10)")),
    e("skills.MoltenShell.preDamageFunc", "act_str.lua", 7599, 7603, "3a02403e817b375d2f14abed327b1bc6e16bec124cc220852b05eab7de952d01", Port(f::molten_shell)),
    e("skills.MoltenStrike.preDamageFunc", "act_str.lua", 7832, 7882, "a54f809c7509641eed7fbd55b08bbf94ea474af7f4161f5a52c61860da4c1c6f", Port(f::molten_strike)),
    e("skills.MoltenStrikeAltX.preDamageFunc", "act_str.lua", 8011, 8082, "1aa115a4b356ed97dea4dc33d1b6a8d907005bac936b9870758a4a51ec7e5c69", Port(f::molten_strike_alt_x)),
    e("skills.OrbOfStorms.preDamageFunc", "act_int.lua", 13621, 13623, "c16984d1b006d2c64d68d32bf1b6b473c2897c50d2af1f7fd72f7629c53afc60", Port(f::orb_of_storms)),
    e("skills.PenanceBrand.preDamageFunc", "act_int.lua", 13809, 13811, "d4facd0432a4e07b57b66a47fd452a9a425261e509f7f3dd98c8f732947a8476", Port(f::penance_brand)),
    e("skills.PenanceBrandAltX.preDamageFunc", "act_int.lua", 13901, 13903, "48907eac3b6310cb369ef87ebddefa1a756bd1661da336193b0eaae2df3d17c6", Port(f::brand_activation)),
    e("skills.PenanceBrandAltY.preDamageFunc", "act_int.lua", 14015, 14017, "48907eac3b6310cb369ef87ebddefa1a756bd1661da336193b0eaae2df3d17c6", Port(f::brand_activation)),
    e("skills.Perforate.preDamageFunc", "act_str.lua", 8304, 8306, "14405ec5edd00d5194d9589ea73b42a0096d422d8fb6bacc1ec9b4e4cb4ff8c6", Port(f::perforate)),
    e("skills.PerforateAltX.preDamageFunc", "act_str.lua", 8400, 8402, "14405ec5edd00d5194d9589ea73b42a0096d422d8fb6bacc1ec9b4e4cb4ff8c6", Port(f::perforate)),
    e("skills.PerforateAltY.preDamageFunc", "act_str.lua", 8501, 8503, "14405ec5edd00d5194d9589ea73b42a0096d422d8fb6bacc1ec9b4e4cb4ff8c6", Port(f::perforate)),
    e("skills.PoisonousConcoction.preDamageFunc", "act_dex.lua", 17423, 17428, "558a8faca69370fdd764591fcaa080a6035da9236776ff310c8c3ac990b7009e", Port(f::poisonous_concoction)),
    e("skills.PoisonousConcoctionAltX.preDamageFunc", "act_dex.lua", 17524, 17529, "558a8faca69370fdd764591fcaa080a6035da9236776ff310c8c3ac990b7009e", Port(f::poisonous_concoction)),
    e("skills.RageVortex.preSkillTypeFunc", "act_str.lua", 9082, 9091, "3c8202d2061670642148688e8d08947e4e8a3124e115251dc7861020e5dc1146", Port(f::rage_vortex)),
    e("skills.RighteousFire.preDamageFunc", "act_int.lua", 15428, 15434, "47f63f828c7e4767e698ee0c010aa605ced004ea8c0a7b486d7aaa75b3c82225", Port(f::righteous_fire)),
    e("skills.RighteousFireAltX.preDamageFunc", "act_int.lua", 15533, 15537, "12dd2781a200d312c2fb21b2cc18b0478792af63dd2581d67a2ad6d207ee775b", Port(f::righteous_fire_alt_x)),
    e("skills.ScourgeArrow.preDamageFunc", "act_dex.lua", 12990, 12992, "f15cf01aee5a4b61d1d80cffe7e621ee4a0d31b1d680b1e54bf7a2d91ea8fa16", Port(f::scourge_arrow)),
    e("skills.SeismicTrap.preDamageFunc", "act_dex.lua", 13381, 13496, "bcab0d881fb82cd7cab01ece5aa881433d7694c83538ffb0c4af3399d14a8b6e", Port(f::seismic_trap)),
    e("skills.SeismicTrapAltX.preDamageFunc", "act_dex.lua", 13622, 13671, "9a6bb655aa4aa81df0ceeaa8b090d8fa397c8bddff395a52765e38189680357a", Port(f::seismic_trap_alt_x)),
    e("skills.ShrapnelBallista.preDamageFunc", "act_dex.lua", 13779, 13787, "a5e0e6526ed2da2039a231401f2141ba83c32b20a773365c8e069c59945796a1", Port(f::shrapnel_ballista)),
    e("skills.ShrapnelBallistaAltX.preDamageFunc", "act_dex.lua", 13867, 13875, "a5e0e6526ed2da2039a231401f2141ba83c32b20a773365c8e069c59945796a1", Port(f::shrapnel_ballista)),
    e("skills.Spark.preDamageFunc", "act_int.lua", 16721, 16727, "3eda1a3a1b56279d7349ca9f484f7889c383b67346dcbb78c9cd19d9ce3f887e", Port(f::spark)),
    e("skills.SparkAltX.preDamageFunc", "act_int.lua", 16811, 16817, "6ba236c5d8dff7afa2ebf95e25c04b32216a8b592594ca233b11303f93b36096", Port(f::spark)),
    e("skills.SparkAltY.preDamageFunc", "act_int.lua", 16902, 16908, "3eda1a3a1b56279d7349ca9f484f7889c383b67346dcbb78c9cd19d9ce3f887e", Port(f::spark)),
    e("skills.StaticStrike.preDamageFunc", "act_str.lua", 10726, 10730, "30f986590c9224a1fd7f329b289eca6159a4a7058fbb725a25311d8b767fdfcb", Port(f::static_strike)),
    e("skills.StormBrand.preDamageFunc", "act_int.lua", 17373, 17375, "48907eac3b6310cb369ef87ebddefa1a756bd1661da336193b0eaae2df3d17c6", Port(f::brand_activation)),
    e("skills.StormBrandAltX.preDamageFunc", "act_int.lua", 17470, 17472, "48907eac3b6310cb369ef87ebddefa1a756bd1661da336193b0eaae2df3d17c6", Port(f::brand_activation)),
    e("skills.StormBurst.preDamageFunc", "act_int.lua", 18036, 18048, "3cec102ee122f79dbd58a5023871dadda66a8edf257e1c49be4461d59f0dad94", Port(f::storm_burst)),
    e("skills.StormBurstAltX.preDamageFunc", "act_int.lua", 18151, 18157, "41a3cdc39bf20760e409a1933f6e52f7c5cb66a52b830d12a785239596d3fce0", Port(f::storm_burst_alt_x)),
    e("skills.StormRain.preDamageFunc", "act_dex.lua", 11747, 11752, "b2496d03fd13d5fb90bf078a953fe41c6913b0099fbd2b3def79e79b04ab5066", Port(f::storm_rain)),
    e("skills.StormRainAltX.preDamageFunc", "act_dex.lua", 11851, 11856, "6e93f9cb7d7c788e8ac3129cbfbd179fde996179284eda25b965c5a6c6a76d98", Port(f::storm_rain_alt_x)),
    e("skills.StormRainAltY.preDamageFunc", "act_dex.lua", 11958, 11964, "c1234ad1dc0fa6652c350cf0b5c135a3642bc721047bc832b318b62505e30859", Port(f::storm_rain_alt_y)),
    e("skills.Tornado.preDamageFunc", "act_dex.lua", 18034, 18036, "91c37fecdf57afb92b32b98c4ed67f684fcdc47584e97d79c02c5e259873c87b", Port(f::tornado)),
    e("skills.TornadoAltY.preDamageFunc", "act_dex.lua", 18127, 18130, "62e81aaabd74e61697a16e66ec3c776e095d5b638e71b186bc42ba5b00b3eec2", Port(f::tornado_alt_y)),
    e("skills.TornadoShot.preDamageFunc", "act_dex.lua", 16051, 16062, "d13aed30ef6bb210c32448cd93f0332c18e4892227ad62bde08547881701e2a0", Port(f::tornado_shot)),
    e("skills.ToxicRain.preDamageFunc", "act_dex.lua", 14935, 14937, "a607479cf006b5e38a533faf7759bf4c720bf6f0dcd325bbf27566aaedd92ca2", Port(f::toxic_rain)),
    e("skills.ToxicRainAltX.preDamageFunc", "act_dex.lua", 15027, 15029, "a607479cf006b5e38a533faf7759bf4c720bf6f0dcd325bbf27566aaedd92ca2", Port(f::toxic_rain)),
    e("skills.ToxicRainAltY.preDamageFunc", "act_dex.lua", 15116, 15118, "a607479cf006b5e38a533faf7759bf4c720bf6f0dcd325bbf27566aaedd92ca2", Port(f::toxic_rain)),
    e("skills.VaalBladeVortex.preDamageFunc", "act_dex.lua", 2428, 2430, "155ceb3b36862cb3a1fd5c046642cb437f354b1d9f612577a85e49a4608edd9d", Port(f::vaal_blade_vortex)),
    e("skills.VaalCyclone.initialFunc", "act_dex.lua", 4934, 4947, "a17ea50f272107e6da0976d59b0338a9c02946a975073f5d1bf3dac7ee02727f", Port(f::cyclone)),
    e("skills.VaalLightningArrow.preDamageFunc", "act_dex.lua", 10590, 10594, "659342bba434fa868ca4478a8ef9e416bb275f9da00e180d681b210af698ca1b", Port(f::part2_dps_eq_projectile_count)),
    e("skills.VaalMoltenShell.preDamageFunc", "act_str.lua", 7701, 7705, "87435ec8cd27eb64b0dde5d339e393bc127cbd034c313309a284591744107c8f", Port(f::vaal_molten_shell)),
    e("skills.VaalRighteousFire.preDamageFunc", "act_int.lua", 15625, 15631, "00b0c5422a4ecb05b7f078f5c7b1d3407624dea66fa3ae4b73a5d88da4db2e78", Port(f::vaal_righteous_fire)),
    e("skills.VoidSphere.preDamageFunc", "act_int.lua", 20267, 20269, "0fe6add8a42008e102b8e4143c3e5813ba32f1a423d639b6750f727f8b791a51", Port(f::void_sphere)),
    e("skills.VoidSphereAltX.preDamageFunc", "act_int.lua", 20356, 20358, "0fe6add8a42008e102b8e4143c3e5813ba32f1a423d639b6750f727f8b791a51", Port(f::void_sphere)),
    e("skills.VoltaxicBurst.preDamageFunc", "act_int.lua", 20445, 20448, "7491b90332deaa6b87be2e67058f16c82da7744dd32c0cee0f3d94e161959927", Port(f::voltaxic_burst)),
    e("skills.Vortex.preDamageFunc", "act_int.lua", 20542, 20544, "bcb877cd38f680213da072ab311e27cfa9cb87d1faa786617e844738aad9537e", Port(f::hit_time_cooldown)),
    e("skills.VortexAltX.preDamageFunc", "act_int.lua", 20629, 20631, "bcb877cd38f680213da072ab311e27cfa9cb87d1faa786617e844738aad9537e", Port(f::hit_time_cooldown)),
    e("skills.WaveOfConviction.preDamageFunc", "act_int.lua", 20795, 20800, "d4d6595d68b37dd6fa2c629d0136a52a372db77a256afb356d3486a592f28e4b", Port(f::wave_of_conviction)),
    e("skills.WaveOfConvictionAltY.preDamageFunc", "act_int.lua", 20889, 20894, "b68cf4116a44f96ab8e673e4803e33a87a20f04fd98e48b36662ec499174edac", Port(f::wave_of_conviction_alt_y)),
    e("skills.WinterOrb.preDamageFunc", "act_int.lua", 20983, 20987, "ffece993f623ae6cf65174c43bc24ec134484057f5836476506381d9332d10a5", Port(f::winter_orb)),
    e("skills.WintertideBrand.preDamageFunc", "act_int.lua", 21107, 21131, "a78ef7b6cb1c7bef2be331229a14f7fd2121b407f0f0e1b3c64454d8c1723e35", Port(f::wintertide_brand)),
];

/// `activeSkill.activeEffect.grantedEffect[hook](activeSkill, output, breakdown)` for the effect `id`.
pub(crate) fn call(off: &mut Off<'_>, id: &str, hook: &str) {
    let path = format!("skills.{id}.{hook}");
    let Some(en) = SKILL_FUNCS.iter().find(|e| e.path == path) else {
        panic!("Data/Skills: {path} is a hook of the granted effect but not in data/closures.json");
    };
    match en.body {
        Body::Port(func) => func(off),
        Body::Unported(why) => {
            off.note(format!("Data/Skills/{}:{}-{} {path}: {why}", en.src, en.from, en.to));
        }
    }
}

/// Every registry entry as `(path, src under src/Data/Skills/, from, to, sha256, kind)` with kind `"port"` / `"unported"`
/// (the drift test compares this with `data/closures.json`).
pub fn skill_func_entries() -> Vec<(&'static str, &'static str, u32, u32, &'static str, &'static str)> {
    SKILL_FUNCS
        .iter()
        .map(|e| {
            let kind = match e.body {
                Body::Port(_) => "port",
                Body::Unported(_) => "unported",
            };
            (e.path, e.src, e.from, e.to, e.sha, kind)
        })
        .collect()
}

/// `(ported, unported, later)` entry counts (`later` -- hooks called from offence sections not ported yet: none since R7.5), and the
/// `<src>:<from>-<to> <path>` of every unported one.
pub fn skill_func_summary() -> (usize, usize, usize, Vec<String>) {
    let (mut p, mut u, l) = (0, 0, 0);
    let mut names = Vec::new();
    for en in &SKILL_FUNCS {
        match en.body {
            Body::Port(_) => p += 1,
            Body::Unported(why) => {
                u += 1;
                names.push(format!("{}:{}-{} {}: {why}", en.src, en.from, en.to, en.path));
            }
        }
    }
    (p, u, l, names)
}
