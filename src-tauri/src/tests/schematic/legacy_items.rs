use super::*;

#[test]
fn listed_nested_items_use_selected_replacements_and_discs_remain_markers() {
    use crate::schematic::data_replacements::{self,Choice,Kind};
    let log=stack("minecraft:pale_oak_log",32,c(vec![("minecraft:custom_name",text("\"材料\""))]));
    let box_item=stack("minecraft:shulker_box",1,c(vec![("minecraft:container",Value::List(vec![c(vec![
        ("slot",Value::Int(5)),("item",log),
    ])],10))]));
    let input=root(vec![c(vec![("Items",Value::List(vec![box_item,stack("minecraft:music_disc_creator",1,c(vec![]))],10))])]);
    let rows=data_replacements::analyze(&input,4189,3700).unwrap();
    assert_eq!(rows.len(),2);
    let logs=rows.iter().find(|r|r.name=="minecraft:pale_oak_log").unwrap();
    assert_eq!(logs.count,32);
    let choices=vec![Choice{kind:Kind::Item,name:logs.name.clone(),replacement:"minecraft:spruce_log".into()},
        Choice{kind:Kind::Item,name:"minecraft:music_disc_creator".into(),replacement:data_replacements::MARKER.into()}];
    let result=prepare_with_choices(&input,4189,3700,Some(&choices)).unwrap().unwrap();
    assert_eq!(result.replacements.len(),2);
    let items=result.root.get("Regions").unwrap().get("r").unwrap().get("TileEntities").unwrap().as_list().unwrap()[0].get("Items").unwrap().as_list().unwrap();
    let nested=&items[0].get("tag").unwrap().get("BlockEntityTag").unwrap().get("Items").unwrap().as_list().unwrap()[0];
    assert_eq!(nested.get("id"),Some(&text("minecraft:spruce_log")));
    assert_eq!(nested.get("Count"),Some(&Value::Byte(32)));
    assert_eq!(nested.get("Slot"),Some(&Value::Byte(5)));
    assert_eq!(nested.get("tag").unwrap().get("display").unwrap().get("Name"),Some(&text("\"材料\"")));
    assert_eq!(items[1].get("id"),Some(&text(data_replacements::MARKER)));
    assert!(items[1].get("tag").unwrap().get("Enchantments").is_some());
    let before=input.clone();
    assert!(prepare_with_choices(&input,4189,3700,Some(&choices[..1])).is_err());
    let mut invalid=choices.clone();invalid[1].replacement="minecraft:music_disc_13".into();
    assert!(prepare_with_choices(&input,4189,3700,Some(&invalid)).is_err());
    assert_eq!(input,before);
}

#[test]
fn marking_a_filled_container_rejects_instead_of_ignoring_inner_choices() {
    use crate::schematic::data_replacements::{self,Choice};
    let outer=stack("minecraft:pale_oak_chest_boat",1,c(vec![("minecraft:container",Value::List(vec![c(vec![
        ("slot",Value::Int(0)),("item",stack("minecraft:music_disc_creator",1,c(vec![]))),
    ])],10))]));
    let input=root(vec![c(vec![("Items",Value::List(vec![outer],10))])]);
    let before=input.clone();
    let mut choices:Vec<_>=data_replacements::analyze(&input,4189,3700).unwrap().into_iter()
        .map(|row|Choice {kind:row.kind,name:row.name,replacement:data_replacements::MARKER.into()}).collect();
    let error=prepare_with_choices(&input,4189,3700,Some(&choices)).unwrap_err().to_string();
    assert!(error.contains("内部选择无法生效"));
    choices.iter_mut().find(|row|row.name=="minecraft:pale_oak_chest_boat").unwrap().replacement="minecraft:stick".into();
    assert!(prepare_with_choices(&input,4189,3700,Some(&choices)).unwrap_err().to_string().contains("库存迁移规则"));
    let nested=c(vec![("SpawnPotentials",Value::List(vec![c(vec![("data",c(vec![("entity",c(vec![
        ("Passengers",Value::List(vec![c(vec![("HandItems",Value::List(vec![stack("minecraft:stone",1,c(vec![]))],10))])],10)),
    ]))]))])],10))]);
    let outer=stack("minecraft:pale_oak_chest_boat",1,c(vec![("minecraft:block_entity_data",nested)]));
    let embedded=root(vec![c(vec![("Items",Value::List(vec![outer],10))])]);
    assert!(prepare(&embedded,4189,3700).unwrap_err().to_string().contains("库存迁移规则"));
    assert!(error.contains("Items[0]"));
    assert_eq!(input,before);
}

#[test]
fn pale_boats_are_listed_separately_and_keep_passengers_position_and_inventory() {
    use crate::schematic::data_replacements::{self,Choice,Kind};
    let position=Value::List(vec![Value::Double(1.5),Value::Double(4.0),Value::Double(-2.5)],6);
    let boat=c(vec![("id",text("minecraft:pale_oak_chest_boat")),("Pos",position.clone()),
        ("Items",Value::List(vec![stack("minecraft:pale_oak_log",6,c(vec![]))],10)),
        ("Passengers",Value::List(vec![c(vec![("id",text("minecraft:pale_oak_boat"))])],10))]);
    let input=c(vec![("Regions",c(vec![("r",c(vec![("Entities",Value::List(vec![boat],10))]))]))]);
    let rows=data_replacements::analyze(&input,4189,3700).unwrap();assert_eq!(rows.len(),3);
    let choices:Vec<_>=rows.iter().map(|r|Choice{kind:r.kind,name:r.name.clone(),replacement:match r.kind {
        Kind::Item=>"minecraft:oak_log".into(),
        Kind::Entity=>if r.name.ends_with("chest_boat") {"minecraft:chest_boat[Type=cherry]"} else {"minecraft:boat[Type=spruce]"}.into(),
    }}).collect();
    let result=prepare_with_choices(&input,4189,3700,Some(&choices)).unwrap().unwrap();
    assert_eq!(result.replacements.len(),3);
    let boat=&result.root.get("Regions").unwrap().get("r").unwrap().get("Entities").unwrap().as_list().unwrap()[0];
    assert_eq!(boat.get("id"),Some(&text("minecraft:chest_boat")));
    assert_eq!(boat.get("Type"),Some(&text("cherry")));
    assert_eq!(boat.get("Pos"),Some(&position));
    assert_eq!(boat.get("Items").unwrap().as_list().unwrap()[0].get("id"),Some(&text("minecraft:oak_log")));
    assert_eq!(boat.get("Passengers").unwrap().as_list().unwrap()[0].get("Type"),Some(&text("spruce")));
}

#[test]
fn sword_keeps_unbreakable_and_sweeping_level() {
    let mut item = stack("minecraft:netherite_sword", 1, c(vec![
        ("minecraft:unbreakable", c(vec![])),
        ("minecraft:enchantments", c(vec![("levels", c(vec![("minecraft:sweeping_edge", Value::Int(3))]))])),
    ]));
    downgrade_item(&mut item, "sword").unwrap();
    let tag = item.get("tag").unwrap();
    assert_eq!(tag.get("Unbreakable"), Some(&Value::Byte(1)));
    assert_eq!(tag.get("Enchantments"), Some(&Value::List(vec![c(vec![
        ("id", text("minecraft:sweeping")), ("lvl", Value::Short(3)),
    ])], 10)));
    let mut hidden = stack("minecraft:stone", 1, c(vec![
        ("minecraft:custom_data", c(vec![("HideFlags", Value::Int(1))])),
        ("minecraft:unbreakable", c(vec![("show_in_tooltip", Value::Byte(0))])),
    ]));
    downgrade_item(&mut hidden, "hidden").unwrap();
    assert_eq!(hidden.get("tag").unwrap().get("HideFlags"), Some(&Value::Int(5)));
}

#[test]
fn writable_pages_keep_plain_text_line_breaks_and_filtered_variants() {
    let mut book = stack("minecraft:writable_book", 1, c(vec![
        ("minecraft:writable_book_content", c(vec![("pages", Value::List(vec![
            c(vec![("raw", text("说明\n11gt攻击")), ("filtered", text("说明\n过滤文本"))]),
            c(vec![("raw", text("How to stop\nStop spawning."))]),
        ], 10))])),
    ]));
    downgrade_item(&mut book, "book").unwrap();
    let tag = book.get("tag").unwrap();
    assert_eq!(tag.get("pages"), Some(&Value::List(vec![text("说明\n11gt攻击"), text("How to stop\nStop spawning.")], 8)));
    assert_eq!(tag.get("filtered_pages"), Some(&c(vec![("0", text("说明\n过滤文本"))])));
}

#[test]
fn conflicting_book_or_unbreakable_data_rejects_without_changing_root() {
    for data in [
        c(vec![("minecraft:unbreakable", c(vec![])), ("minecraft:custom_data", c(vec![("Unbreakable", Value::Byte(0))]))]),
        c(vec![("minecraft:writable_book_content", c(vec![("pages", Value::List(vec![c(vec![("raw", text("new"))])], 10))])),
            ("minecraft:custom_data", c(vec![("pages", Value::List(vec![text("old")], 8))]))]),
        c(vec![("minecraft:enchantments", c(vec![("levels", c(vec![("minecraft:sweeping", Value::Int(2)), ("minecraft:sweeping_edge", Value::Int(3))]))]))]),
        c(vec![("minecraft:writable_book_content", c(vec![("pages", Value::List(vec![c(vec![("raw", text("new"))])], 10))])),
            ("minecraft:custom_data", c(vec![("filtered_pages", c(vec![("0", text("old filtered"))]))]))]),
    ] {
        let input = root(vec![c(vec![("Items", Value::List(vec![stack("minecraft:stone", 1, data)], 10))])]);
        let before = input.clone();
        assert!(prepare(&input, 3953, 3700).is_err());
        assert_eq!(input, before);
    }
}
fn downgrade_item(value: &mut Value, path: &str) -> AppResult<()> {
    super::downgrade_item(value, path, &mut Migration::default())
}
fn downgrade_contents(value: &mut Value, path: &str) -> AppResult<()> {
    super::downgrade_contents(value, path, &mut Migration::default())
}

fn c(fields: Vec<(&str, Value)>) -> Value {
    Value::Compound(fields.into_iter().map(|(k, v)| (k.into(), v)).collect())
}

fn text(value: &str) -> Value { Value::String(value.into()) }

fn stack(id: &str, count: i32, components: Value) -> Value {
    c(vec![("id", text(id)), ("count", Value::Int(count)), ("components", components)])
}

fn root(entries: Vec<Value>) -> Value {
    c(vec![
        ("Version", Value::Int(7)),
        ("MinecraftDataVersion", Value::Int(3953)),
        ("Regions", c(vec![("r", c(vec![
            ("TileEntities", Value::List(entries, 10)),
        ]))])),
    ])
}

#[test]
fn nested_container_keeps_slots_counts_and_display_data() {
    let nested = stack("minecraft:bone_meal", 41, c(vec![
        ("minecraft:lore", Value::List(vec![text("\"说明\"")], 8)),
    ]));
    let mut outer = stack("minecraft:shulker_box", 1, c(vec![
        ("minecraft:custom_name", text("\"补给\"")),
        ("minecraft:container", Value::List(vec![c(vec![
            ("slot", Value::Int(3)), ("item", nested),
        ])], 10)),
    ]));
    outer.set("Slot", Value::Byte(7));
    downgrade_item(&mut outer, "test").unwrap();
    assert_eq!(outer.get("Count"), Some(&Value::Byte(1)));
    assert_eq!(outer.get("Slot"), Some(&Value::Byte(7)));
    assert!(outer.get("components").is_none());
    let tag = outer.get("tag").unwrap();
    assert_eq!(tag.get("display").unwrap().get("Name"), Some(&text("\"补给\"")));
    let inner = &tag.get("BlockEntityTag").unwrap().get("Items").unwrap().as_list().unwrap()[0];
    assert_eq!(inner.get("Slot"), Some(&Value::Byte(3)));
    assert_eq!(inner.get("Count"), Some(&Value::Byte(41)));
    assert_eq!(inner.get("tag").unwrap().get("display").unwrap().get("Lore"),
        Some(&Value::List(vec![text("\"说明\"")], 8)));
}

#[test]
fn frame_and_passenger_inventory_migrate_without_moving_entities() {
    let frame = c(vec![
        ("id", text("minecraft:item_frame")),
        ("TileY", Value::Int(-10)),
        ("Facing", Value::Byte(1)),
        ("Pos", Value::List(vec![Value::Double(13.5), Value::Double(-9.96875), Value::Double(-11.5)], 6)),
        ("Item", stack("minecraft:clock", 1, c(vec![]))),
        ("Passengers", Value::List(vec![c(vec![
            ("id", text("minecraft:hopper_minecart")),
            ("Items", Value::List(vec![stack("minecraft:bone_meal", 64, c(vec![]))], 10)),
        ])], 10)),
    ]);
    let mut converted = frame.clone();
    downgrade_contents(&mut converted, "frame").unwrap();
    for key in ["id", "TileY", "Facing", "Pos"] {
        assert_eq!(frame.get(key), converted.get(key));
    }
    assert_eq!(converted.get("Item").unwrap().get("Count"), Some(&Value::Byte(1)));
    let passenger = &converted.get("Passengers").unwrap().as_list().unwrap()[0];
    assert_eq!(passenger.get("Items").unwrap().as_list().unwrap()[0].get("Count"), Some(&Value::Byte(64)));
}

#[test]
fn signs_and_custom_data_are_not_interpreted_as_item_stacks() {
    let sign = c(vec![
        ("id", text("minecraft:sign")),
        ("front_text", c(vec![("messages", Value::List(vec![text("\"文字\""); 4], 8))])),
    ]);
    let input = root(vec![sign.clone()]);
    let output = prepare(&input, 3953, 3700).unwrap().unwrap().root;
    assert_eq!(output.get("Regions").unwrap().get("r").unwrap().get("TileEntities"),
        Some(&Value::List(vec![sign], 10)));
    let arbitrary = c(vec![("id", text("app:data")), ("count", Value::Int(900))]);
    let mut item = stack("minecraft:stone", 1, c(vec![("minecraft:custom_data", arbitrary.clone())]));
    downgrade_item(&mut item, "test").unwrap();
    assert_eq!(item.get("tag"), Some(&arbitrary));
}

#[test]
fn unknown_components_reject_the_whole_migration_with_location() {
    let known = c(vec![("Items", Value::List(vec![stack("minecraft:stone", 2, c(vec![]))], 10))]);
    let unknown = c(vec![("Items", Value::List(vec![stack("minecraft:stone", 1,
        c(vec![("mod:unknown", Value::Int(1))]))], 10))]);
    let input = root(vec![known, unknown]);
    let before = input.clone();
    let err = prepare(&input, 3953, 3700).unwrap_err().to_string();
    assert!(err.contains("Regions/r/TileEntities[1]/Items[0]/components/mod:unknown"));
    assert_eq!(input, before);
}

#[test]
fn invalid_counts_and_component_collisions_are_rejected() {
    for count in [0, -1, 128, 256] {
        let mut item = stack("minecraft:stone", count, c(vec![]));
        let before = item.clone();
        assert!(downgrade_item(&mut item, "test").is_err());
        assert_eq!(item, before);
    }
    let mut item = stack("minecraft:stone", 1, c(vec![
        ("minecraft:custom_name", text("\"new\"")),
        ("minecraft:custom_data", c(vec![("display", c(vec![("Name", text("\"old\""))]))])),
    ]));
    assert!(downgrade_item(&mut item, "test").unwrap_err().to_string().contains("冲突"));
}

#[test]
fn enchantments_keep_typed_levels_and_existing_items_stay_unchanged() {
    let mut item = stack("minecraft:diamond_pickaxe", 1, c(vec![
        ("minecraft:damage", Value::Int(12)),
        ("minecraft:enchantments", c(vec![("levels", c(vec![("minecraft:efficiency", Value::Int(5))]))])),
    ]));
    downgrade_item(&mut item, "test").unwrap();
    assert_eq!(item.get("tag").unwrap().get("Damage"), Some(&Value::Int(12)));
    assert_eq!(item.get("tag").unwrap().get("Enchantments"), Some(&Value::List(vec![
        c(vec![("id", text("minecraft:efficiency")), ("lvl", Value::Short(5))]),
    ], 10)));
    let before = item.clone();
    downgrade_item(&mut item, "test").unwrap();
    assert_eq!(before, item);
}

#[test]
fn other_version_boundaries_do_not_receive_a_false_success() {
    let input = root(vec![]);
    assert!(prepare(&input, 3700, 3953).unwrap().is_none());
    assert!(prepare(&input, 3953, 3955).unwrap().is_none());
    assert!(prepare(&input, 3953, 3465).is_err());
    assert!(prepare(&input, 4325, 3700).is_err());
}

#[test]
fn unavailable_enchantments_are_rejected_before_saving() {
    let mut item = stack("minecraft:diamond_sword", 1, c(vec![
        ("minecraft:enchantments", c(vec![("levels", c(vec![("minecraft:breach", Value::Int(2))]))])),
    ]));
    assert!(downgrade_item(&mut item, "test").unwrap_err().to_string().contains("breach"));
}

#[test]
fn unavailable_items_preserve_original_stack_and_report_nested_slot_and_position() {
    let original = stack("minecraft:music_disc_creator", 2, c(vec![]));
    let outer = stack("minecraft:shulker_box", 1, c(vec![
        ("minecraft:container", Value::List(vec![c(vec![
            ("slot", Value::Int(0)), ("item", original.clone()),
        ])], 10)),
    ]));
    let mut input = root(vec![c(vec![
        ("id", text("minecraft:chest")), ("x", Value::Int(2)),
        ("y", Value::Int(3)), ("z", Value::Int(4)),
        ("Items", Value::List(vec![outer], 10)),
    ])]);
    let mut region = input.get("Regions").unwrap().get("r").unwrap().clone();
    region.set("Position", c(vec![("x", Value::Int(10)), ("y", Value::Int(27)), ("z", Value::Int(26))]));
    region.set("Size", c(vec![("x", Value::Int(36)), ("y", Value::Int(-28)), ("z", Value::Int(-27))]));
    input.set("Regions", c(vec![("r", region)]));
    let before = input.clone();
    let prepared = prepare(&input, 3953, 3700).unwrap().unwrap();
    assert_eq!(input, before);
    let report = &prepared.replacements[0];
    assert_eq!(prepared.replacements.len(), 1);
    assert_eq!(report.slot, Some(0));
    assert_eq!(report.location, "r / minecraft:chest / (12, 3, 4)");
    assert!(report.path.ends_with("/Items[0]/components/minecraft:container[0]/item"));
    let outer = &prepared.root.get("Regions").unwrap().get("r").unwrap().get("TileEntities").unwrap().as_list().unwrap()[0]
        .get("Items").unwrap().as_list().unwrap()[0];
    let item = &outer.get("tag").unwrap().get("BlockEntityTag").unwrap().get("Items").unwrap().as_list().unwrap()[0];
    assert_eq!(item.get("id"), Some(&text("minecraft:red_stained_glass_pane")));
    assert_eq!(item.get("Count"), Some(&Value::Byte(2)));
    assert_eq!(item.get("Slot"), Some(&Value::Byte(0)));
    let tag = item.get("tag").unwrap();
    let name: serde_json::Value = serde_json::from_str(tag.get("display").unwrap().get("Name").unwrap().as_str().unwrap()).unwrap();
    assert_eq!(name["text"], "音乐唱片：Creator");
    assert_eq!(tag.get("Enchantments").unwrap().as_list().unwrap()[0].get("id"), Some(&text("minecraft:mending")));
    let mut saved_original = original;
    saved_original.set("Slot", Value::Byte(0));
    assert_eq!(tag.get("schematic_manager").unwrap().get("original_item"), Some(&saved_original));
}

#[test]
fn multiple_disc_replacements_report_entity_coordinates_and_each_slot() {
    let ids = ["minecraft:music_disc_creator_music_box", "minecraft:music_disc_creator", "minecraft:music_disc_precipice"];
    let names = ["音乐唱片：Creator（八音盒）", "音乐唱片：Creator", "音乐唱片：Precipice"];
    let items = ids.iter().enumerate().map(|(i, id)| {
        let mut item = stack(id, 1, c(vec![]));
        item.set("Slot", Value::Byte(i as i8));
        item
    }).collect();
    let input = c(vec![("Regions", c(vec![("r", c(vec![
        ("Position", c(vec![("x", Value::Int(10)), ("y", Value::Int(20)), ("z", Value::Int(30))])),
        ("Entities", Value::List(vec![c(vec![
            ("id", text("minecraft:chest_minecart")),
            ("Pos", Value::List(vec![Value::Double(1.5), Value::Double(-2.0), Value::Double(3.5)], 6)),
            ("Items", Value::List(items, 10)),
        ])], 10)),
    ]))]))]);
    let prepared = prepare(&input, 3953, 3700).unwrap().unwrap();
    assert_eq!(prepared.replacements.len(), 3);
    let entity = &prepared.root.get("Regions").unwrap().get("r").unwrap().get("Entities").unwrap().as_list().unwrap()[0];
    for (i, item) in entity.get("Items").unwrap().as_list().unwrap().iter().enumerate() {
        let report = &prepared.replacements[i];
        assert_eq!(report.original_id, ids[i]);
        assert_eq!(report.location, "r / minecraft:chest_minecart / (11.5, 18, 33.5)");
        assert_eq!(report.slot, Some(i as u8));
        let name: serde_json::Value = serde_json::from_str(item.get("tag").unwrap().get("display").unwrap().get("Name").unwrap().as_str().unwrap()).unwrap();
        assert_eq!(name["text"], names[i]);
    }
}

#[test]
fn unsupported_equipment_and_trades_cannot_bypass_component_checks() {
    let mut wolf = c(vec![("body_armor_item", stack("minecraft:leather_chestplate", 1, c(vec![])))]);
    assert!(downgrade_contents(&mut wolf, "wolf").unwrap_err().to_string().contains("body_armor_item"));
    let mut villager = c(vec![("Offers", c(vec![("Recipes", Value::List(vec![c(vec![
        ("sell", stack("minecraft:stone", 1, c(vec![("mod:unknown", Value::Int(1))]))),
    ])], 10))]))]);
    assert!(downgrade_contents(&mut villager, "villager").unwrap_err().to_string().contains("Offers/Recipes[0]/sell"));
}

#[test]
fn block_entity_component_migrates_embedded_inventory() {
    let mut item = stack("minecraft:chest", 1, c(vec![
        ("minecraft:block_entity_data", c(vec![
            ("id", text("minecraft:chest")),
            ("Items", Value::List(vec![stack("minecraft:stone", 32, c(vec![]))], 10)),
        ])),
    ]));
    downgrade_item(&mut item, "chest").unwrap();
    let block = item.get("tag").unwrap().get("BlockEntityTag").unwrap();
    assert_eq!(block.get("id"), Some(&text("minecraft:chest")));
    assert_eq!(block.get("Items").unwrap().as_list().unwrap()[0].get("Count"), Some(&Value::Byte(32)));
}

#[test]
fn version_1_21_4_restores_furnace_fields_and_nested_inventory() {
    let furnace = c(vec![
        ("id", text("minecraft:furnace")),
        ("cooking_time_spent", Value::Int(15)), ("cooking_total_time", Value::Int(200)),
        ("lit_time_remaining", Value::Int(0)), ("lit_total_time", Value::Int(0)),
        ("Items", Value::List(vec![stack("minecraft:iron_ore", 32, c(vec![]))], 10)),
    ]);
    let item = stack("minecraft:furnace", 1, c(vec![("minecraft:block_entity_data", furnace.clone())]));
    let input = root(vec![furnace, c(vec![("Items", Value::List(vec![item], 10))])]);
    let output = prepare(&input, 4189, 3700).unwrap().unwrap().root;
    assert_eq!(output.get("Version"), Some(&Value::Int(6)));
    let entries = output.get("Regions").unwrap().get("r").unwrap().get("TileEntities").unwrap().as_list().unwrap();
    let embedded = entries[1].get("Items").unwrap().as_list().unwrap()[0]
        .get("tag").unwrap().get("BlockEntityTag").unwrap();
    for value in [&entries[0], embedded] {
        assert_eq!(value.get("CookTime"), Some(&Value::Short(15)));
        assert_eq!(value.get("Items").unwrap().as_list().unwrap()[0].get("Count"), Some(&Value::Byte(32)));
    }
}

#[test]
fn version_1_21_4_migrates_boat_passengers_and_rejects_new_entity_types_atomically() {
    let boat = c(vec![
        ("id", text("minecraft:cherry_chest_boat")),
        ("Items", Value::List(vec![stack("minecraft:stone", 6, c(vec![]))], 10)),
        ("Passengers", Value::List(vec![c(vec![("id", text("minecraft:oak_boat"))])], 10)),
    ]);
    let mut input = root(vec![]);
    input.set("Regions", c(vec![("r", c(vec![("Entities", Value::List(vec![boat], 10))]))]));
    let output = prepare(&input, 4189, 3700).unwrap().unwrap().root;
    let boat = &output.get("Regions").unwrap().get("r").unwrap().get("Entities").unwrap().as_list().unwrap()[0];
    assert_eq!(boat.get("id"), Some(&text("minecraft:chest_boat")));
    assert_eq!(boat.get("Type"), Some(&text("cherry")));
    assert_eq!(boat.get("Items").unwrap().as_list().unwrap()[0].get("Count"), Some(&Value::Byte(6)));
    assert_eq!(boat.get("Passengers").unwrap().as_list().unwrap()[0].get("Type"), Some(&text("oak")));
    input.set("Regions", c(vec![("r", c(vec![("Entities", Value::List(vec![c(vec![("id", text("minecraft:creaking"))])], 10))]))]));
    let before = input.clone();
    assert!(prepare(&input, 4189, 3700).unwrap_err().to_string().contains("minecraft:creaking"));
    assert_eq!(input, before);
}

#[test]
fn model_components_accept_only_one_exact_integer_channel() {
    for data in [Value::Int(7), c(vec![("floats", Value::List(vec![Value::Float(7.0)], 5))])] {
        let mut item = stack("minecraft:stone", 1, c(vec![("minecraft:custom_model_data", data)]));
        downgrade_item(&mut item, "model").unwrap();
        assert_eq!(item.get("tag").unwrap().get("CustomModelData"), Some(&Value::Int(7)));
    }
    for data in [
        c(vec![("floats", Value::List(vec![Value::Float(1.5)], 5))]),
        c(vec![("floats", Value::List(vec![Value::Float(1.0), Value::Float(2.0)], 5))]),
        c(vec![("floats", Value::List(vec![Value::Float(f32::NAN)], 5))]),
        c(vec![("floats", Value::List(vec![Value::Float(2147483648.0)], 5))]),
        c(vec![("flags", Value::List(vec![Value::Byte(1)], 1))]),
        c(vec![("strings", Value::List(vec![text("model")], 8))]),
        c(vec![("colors", Value::List(vec![Value::Int(1)], 3))]),
    ] {
        let input = root(vec![c(vec![("Items", Value::List(vec![stack("minecraft:stone", 1,
            c(vec![("minecraft:custom_model_data", data)]))], 10))])]);
        let before = crate::format::nbt::serialize(&input, "").unwrap();
        let err = prepare(&input, 4189, 3700).unwrap_err().to_string();
        assert!(err.contains("minecraft:custom_model_data"));
        assert_eq!(crate::format::nbt::serialize(&input, "").unwrap(), before);
    }
}

#[test]
fn unrepresentable_entity_fields_do_not_partially_change_the_input() {
    let input = root(vec![c(vec![
        ("id", text("minecraft:furnace")), ("x", Value::Int(4)),
        ("y", Value::Int(5)), ("z", Value::Int(6)),
        ("cooking_time_spent", Value::Int(15)), ("lit_time_remaining", Value::Int(0)),
        ("lit_total_time", Value::Int(1600)),
        ("Items", Value::List(vec![stack("minecraft:iron_ore", 32, c(vec![]))], 10)),
    ])]);
    let before = input.clone();
    let error = prepare(&input, 4189, 3700).unwrap_err().to_string();
    assert!(error.contains("lit_total_time"));
    assert!(error.contains("minecraft:furnace"));
    assert_eq!(input, before);
}

#[test]
fn redundant_legacy_inventory_compares_tag_content_without_requiring_key_order() {
    let legacy_item = c(vec![("Slot", Value::Byte(0)), ("id", text("minecraft:shulker_shell")), ("Count", Value::Byte(64))]);
    let legacy_block = c(vec![("id", text("minecraft:shulker_box")), ("Items", Value::List(vec![legacy_item], 10))]);
    for count in [64, 63] {
        let outer = stack("minecraft:shulker_box", 1, c(vec![
            ("minecraft:block_entity_data", legacy_block.clone()),
            ("minecraft:custom_data", c(vec![("BlockEntityTag", legacy_block.clone()), ("VV|Protocol1_20_3To1_20_5", Value::Byte(1))])),
            ("minecraft:container", Value::List(vec![c(vec![
                ("slot", Value::Int(0)), ("item", stack("minecraft:shulker_shell", count, c(vec![]))),
            ])], 10)),
        ]));
        let input = root(vec![c(vec![("Items", Value::List(vec![outer], 10))])]);
        let before = input.clone();
        let result = prepare(&input, 4189, 3700);
        if count == 64 {
            let output = result.unwrap().unwrap().root;
            let tag = output.get("Regions").unwrap().get("r").unwrap().get("TileEntities").unwrap().as_list().unwrap()[0]
                .get("Items").unwrap().as_list().unwrap()[0].get("tag").unwrap();
            assert_eq!(tag.get("BlockEntityTag"), Some(&legacy_block));
            assert_eq!(tag.get("VV|Protocol1_20_3To1_20_5"), Some(&Value::Byte(1)));
        } else {
            assert!(result.unwrap_err().to_string().contains("冲突"));
        }
        assert_eq!(input, before);
    }
}

#[test]
fn spawner_entity_subtrees_receive_the_same_migration_and_rejection() {
    let spawn = c(vec![("entity", c(vec![
        ("id", text("minecraft:zombie")),
        ("HandItems", Value::List(vec![stack("minecraft:stone", 1, c(vec![])), c(vec![])], 10)),
    ]))]);
    let make_input = |data: Value| root(vec![c(vec![
        ("id", text("minecraft:mob_spawner")), ("SpawnData", spawn.clone()),
        ("SpawnPotentials", Value::List(vec![c(vec![("weight", Value::Int(1)), ("data", data)])], 10)),
    ])]);
    let input = make_input(spawn.clone());
    let output = prepare(&input, 4189, 3700).unwrap().unwrap().root;
    let spawner = &output.get("Regions").unwrap().get("r").unwrap().get("TileEntities").unwrap().as_list().unwrap()[0];
    let entity = spawner.get("SpawnData").unwrap().get("entity").unwrap();
    assert_eq!(entity.get("HandItems").unwrap().as_list().unwrap()[0].get("Count"), Some(&Value::Byte(1)));
    for data in [
        c(vec![("entity", c(vec![("id", text("minecraft:creaking"))]))]),
        c(vec![("entity", c(vec![("id", text("minecraft:zombie")), ("attributes", Value::List(vec![c(vec![])], 10))]))]),
        c(vec![("entity", c(vec![("id", text("minecraft:zombie"))])), ("equipment", c(vec![("loot_table", text("minecraft:equipment/trial_chamber"))]))]),
    ] {
        let input = make_input(data);
        let before = input.clone();
        assert!(prepare(&input, 4189, 3700).unwrap_err().to_string().contains("SpawnPotentials[0]/data"));
        assert_eq!(input, before);
    }
}
