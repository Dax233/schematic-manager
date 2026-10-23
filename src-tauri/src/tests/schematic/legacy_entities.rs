use super::*;

#[test]
fn base_attributes_keep_values_and_reject_modifiers_and_unknown_names() {
    for name in ["minecraft:generic.movement_speed", "minecraft:movement_speed"] {
        let attr = Value::Compound(vec![("id".into(), Value::String(name.into())), ("base".into(), Value::Double(0.7))]);
        let mut entity = c("armor_stand", vec![("attributes", Value::List(vec![attr], 10))]);
        downgrade(&mut entity, "stand").unwrap();
        let attr = &entity.get("Attributes").unwrap().as_list().unwrap()[0];
        assert_eq!(attr.get("Name"), Some(&Value::String("minecraft:generic.movement_speed".into())));
        assert_eq!(attr.get("Base"), Some(&Value::Double(0.7)));
        assert!(entity.get("attributes").is_none());
    }
    for name in ["minecraft:generic.jump_strength", "minecraft:jump_strength"] {
        let mut entity = c("armor_stand", vec![("attributes", Value::List(vec![Value::Compound(vec![
            ("id".into(), Value::String(name.into())), ("base".into(), Value::Double(0.8)),
        ])], 10))]);
        assert!(downgrade(&mut entity, "stand").is_err());
    }
    let mut conflict = c("armor_stand", vec![
        ("attributes", Value::List(vec![], 10)),
        ("Attributes", Value::List(vec![Value::Compound(vec![])], 10)),
    ]);
    assert!(downgrade(&mut conflict, "conflict").is_err());
    for name in ["minecraft:generic.movement_speed", "minecraft:scale"] {
        let mut attr = Value::Compound(vec![("id".into(), Value::String(name.into())), ("base".into(), Value::Double(0.7))]);
        if name.contains("movement_speed") { attr.set("modifiers", Value::List(vec![Value::Compound(vec![])], 10)); }
        assert!(downgrade(&mut c("armor_stand", vec![("attributes", Value::List(vec![attr], 10))]), "stand").is_err());
    }
}

fn c(id: &str, fields: Vec<(&str, Value)>) -> Value {
    let mut value = Value::Compound(fields.into_iter().map(|(k, v)| (k.into(), v)).collect());
    value.set("id", Value::String(format!("minecraft:{id}")));
    value
}

#[test]
fn furnace_timers_are_restored_as_shorts() {
    for id in ["furnace", "smoker", "blast_furnace"] {
        let mut value = c(id, vec![
            ("cooking_time_spent", Value::Int(17)),
            ("cooking_total_time", Value::Int(200)),
            ("lit_time_remaining", Value::Int(0)),
            ("lit_total_time", Value::Int(0)),
        ]);
        downgrade(&mut value, "furnace").unwrap();
        for (key, expected) in [("CookTime", 17), ("CookTimeTotal", 200), ("BurnTime", 0)] {
            assert_eq!(value.get(key), Some(&Value::Short(expected)));
        }
        for key in ["cooking_time_spent", "cooking_total_time", "lit_time_remaining", "lit_total_time"] {
            assert!(value.get(key).is_none());
        }
    }
}

#[test]
fn timers_reject_conflicts_overflow_and_unrepresentable_fuel_totals() {
    for fields in [
        vec![("cooking_time_spent", Value::Int(40000))],
        vec![("cooking_time_spent", Value::Int(5)), ("CookTime", Value::Short(6))],
        vec![("lit_total_time", Value::Int(1600))],
        vec![("lit_total_time", Value::Int(0)), ("lit_time_remaining", Value::Int(100))],
    ] {
        let mut value = c("furnace", fields);
        assert!(downgrade(&mut value, "furnace").is_err());
    }
}

#[test]
fn wood_specific_boats_restore_type_without_moving_them() {
    for (id, target, wood) in [
        ("oak_boat", "boat", "oak"),
        ("dark_oak_chest_boat", "chest_boat", "dark_oak"),
        ("bamboo_raft", "boat", "bamboo"),
        ("bamboo_chest_raft", "chest_boat", "bamboo"),
    ] {
        let pos = Value::List(vec![Value::Double(1.5), Value::Double(20.0), Value::Double(-5.5)], 6);
        let mut value = c(id, vec![("Pos", pos.clone())]);
        downgrade(&mut value, "boat").unwrap();
        validate_type(&value, "boat").unwrap();
        assert_eq!(value.get("id"), Some(&Value::String(format!("minecraft:{target}"))));
        assert_eq!(value.get("Type"), Some(&Value::String(wood.into())));
        assert_eq!(value.get("Pos"), Some(&pos));
    }
    assert!(downgrade(&mut c("pale_oak_boat", vec![]), "boat").is_err());
    assert!(validate_type(&c("creaking", vec![]), "entity").is_err());
}

#[test]
fn existing_boat_types_and_inventory_remain_unchanged() {
    for id in ["boat", "chest_boat"] {
        let original = c(id, vec![
            ("Type", Value::String("oak".into())), ("Items", Value::List(vec![], 10)),
        ]);
        let mut value = original.clone();
        downgrade(&mut value, "boat").unwrap();
        validate_type(&value, "boat").unwrap();
        assert_eq!(value, original);
    }
}

#[test]
fn tnt_minecart_preserves_fuse_and_rejects_nondefault_explosions() {
    let mut value = c("tnt_minecart", vec![
        ("fuse", Value::Int(42)), ("explosion_speed_factor", Value::Float(1.0)),
        ("explosion_power", Value::Float(4.0)),
    ]);
    downgrade(&mut value, "cart").unwrap();
    assert_eq!(value.get("TNTFuse"), Some(&Value::Int(42)));
    assert!(value.get("fuse").is_none());
    for (key, value) in [("explosion_speed_factor", Value::Float(2.0)), ("explosion_power", Value::Float(6.0))] {
        assert!(downgrade(&mut c("tnt_minecart", vec![(key, value)]), "cart").is_err());
    }
}

#[test]
fn unrepresentable_behaviors_report_the_field_path() {
    for (field, data) in [
        ("lock", Value::Compound(vec![])),
        ("attributes", Value::List(vec![Value::Compound(vec![])], 10)),
        ("acceleration_power", Value::Double(0.1)),
    ] {
        let error = downgrade(&mut c("chest", vec![(field, data)]), "Regions/r/entity").unwrap_err().to_string();
        assert!(error.contains(&format!("Regions/r/entity/{field}")));
    }
}

#[test]
fn renamed_entity_data_is_rejected_before_the_target_can_ignore_it() {
    for (id, key) in [
        ("banner", "patterns"), ("skull", "profile"), ("skull", "custom_name"),
        ("beehive", "bees"), ("beehive", "flower_pos"), ("bee", "hive_pos"),
        ("end_gateway", "exit_portal"), ("end_crystal", "beam_target"),
        ("area_effect_cloud", "potion_contents"), ("cow", "leash"),
        ("pillager", "patrol_target"), ("wandering_trader", "wander_target"),
    ] {
        let error = downgrade(&mut c(id, vec![(key, Value::Compound(vec![]))]), "entity").unwrap_err().to_string();
        assert!(error.contains(&format!("entity/{key}")));
    }
    let arrow = c("arrow", vec![("item", c("tipped_arrow", vec![]))]);
    assert!(downgrade(&mut arrow.clone(), "arrow").is_err());
}

#[test]
fn block_entity_custom_metadata_never_replaces_current_inventory() {
    let metadata = Value::Compound(vec![("minecraft:custom_data".into(), Value::Compound(vec![
        ("BlockEntityTag".into(), Value::Compound(vec![("Items".into(), Value::List(vec![], 10))])),
    ]))]);
    let original = c("shulker_box", vec![
        ("Items", Value::List(vec![c("stone", vec![("Count", Value::Byte(1))])], 10)),
        ("components", metadata),
    ]);
    let mut value = original.clone();
    downgrade(&mut value, "box").unwrap();
    assert_eq!(value, original);
    value.set("components", Value::Compound(vec![("minecraft:custom_name".into(), Value::String("\"命名\"".into()))]));
    assert!(downgrade(&mut value, "box").unwrap_err().to_string().contains("minecraft:custom_name"));
}
