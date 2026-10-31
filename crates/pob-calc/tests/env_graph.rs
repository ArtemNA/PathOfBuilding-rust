//! R3-ARCH §2.2: the actor graph of an `Env` (player <-> enemy, `store.actor`, `actor.enemy/player`) leaks nothing --
//! after `drop(env)` no actor, actor table or store is reachable.

use std::rc::Rc;

use pob_build::load_build_xml;
use pob_calc::{InitEnvOptions, init_env};
use pob_golden::registry::GOLDEN_BUILDS;

#[test]
fn env_drop_frees_the_actor_graph() {
    for g in GOLDEN_BUILDS {
        let model = Rc::new(load_build_xml(&std::fs::read_to_string(g.xml_path()).unwrap()).unwrap());
        let env = init_env(model, InitEnvOptions::default()).unwrap();

        // The links work while the env is alive.
        let player = env.player.borrow();
        let enemy = player.enemy.upgrade().expect("player.enemy");
        assert!(Rc::ptr_eq(&enemy, &env.enemy), "{}", g.name);
        assert!(Rc::ptr_eq(&enemy.borrow().player.upgrade().unwrap(), &env.player));
        assert!(Rc::ptr_eq(&env.mod_db.borrow().actor(), &player.actor), "modDB.actor = env.player");
        assert!(
            Rc::ptr_eq(&env.enemy_db.borrow().actor(), &enemy.borrow().actor),
            "enemyDB.actor = env.enemy"
        );
        assert!(
            Rc::ptr_eq(&player.actor.borrow().actors["enemy"], &enemy.borrow().actor),
            "the mod core's actor.enemy"
        );
        drop(enemy);
        // Env (+ the CalcActor's own `player` back-ref is Weak): exactly one strong owner of each CalcActor.
        assert_eq!(Rc::strong_count(&env.player), 1, "{}", g.name);
        assert_eq!(Rc::strong_count(&env.enemy), 1, "{}", g.name);

        let weak = (
            Rc::downgrade(&env.player),
            Rc::downgrade(&env.enemy),
            Rc::downgrade(&player.actor),
            Rc::downgrade(&env.enemy.borrow().actor),
            Rc::downgrade(&env.mod_db),
            Rc::downgrade(&env.enemy_db),
            Rc::downgrade(&env.item_mod_db),
            Rc::downgrade(&env.initial_node_mod_db),
        );
        drop(player);
        drop(env);
        assert!(
            weak.0.upgrade().is_none() && weak.1.upgrade().is_none(),
            "{}: CalcActor leaked",
            g.name
        );
        assert!(
            weak.2.upgrade().is_none() && weak.3.upgrade().is_none(),
            "{}: actor table leaked",
            g.name
        );
        assert!(
            weak.4.upgrade().is_none() && weak.5.upgrade().is_none() && weak.6.upgrade().is_none() && weak.7.upgrade().is_none(),
            "{}: a store leaked",
            g.name
        );
    }
}
