use super::*;
fn c(fields: Vec<(&str,Value)>) -> Value {Value::Compound(fields.into_iter().map(|(k,v)|(k.into(),v)).collect())}
fn text(s: &str) -> Value {Value::String(s.into())}
fn list(items: Vec<Value>) -> Value {Value::List(items,10)}
fn item(id: &str, n: i32) -> Value {c(vec![("id",text(id)),("Count",Value::Byte(n as i8))])}
fn root(tiles: Vec<Value>, entities: Vec<Value>) -> Value {c(vec![("Regions",c(vec![("r",c(vec![("TileEntities",list(tiles)),("Entities",list(entities))]))]))])}

#[test]
fn totals_include_nested_boxes_and_transport_inventory_but_not_display_equipment() {
    let nested=c(vec![("id",text("minecraft:shulker_box")),("Count",Value::Byte(1)),("tag",c(vec![("BlockEntityTag",c(vec![("Items",list(vec![item("minecraft:ender_pearl",16)]))]))]))]);
    let input=root(vec![c(vec![("Items",list(vec![item("minecraft:stone",64),nested]))])],vec![
        c(vec![("id",text("minecraft:chest_minecart")),("Items",list(vec![item("minecraft:stone",32),item("minecraft:diamond_sword",1)]))]),
        c(vec![("id",text("minecraft:arrow")),("item",item("minecraft:arrow",1))]),
        c(vec![("id",text("minecraft:item_frame")),("Item",item("minecraft:stone",64)),("HandItems",list(vec![item("minecraft:stone",64)]))]),
    ]);
    let before=input.clone();let rows=count(&input,3700).unwrap();
    let find=|id:&str|rows.iter().find(|r|r.name==id).unwrap();
    assert_eq!(find("minecraft:stone").count,96);
    assert!((find("minecraft:stone").boxes.unwrap()-96.0/1728.0).abs()<1e-10);
    assert!((find("minecraft:ender_pearl").boxes.unwrap()-16.0/432.0).abs()<1e-10);
    assert!((find("minecraft:diamond_sword").boxes.unwrap()-1.0/27.0).abs()<1e-10);
    assert_eq!(find("minecraft:shulker_box").count,1);
    assert!(!rows.iter().any(|r|r.name=="minecraft:arrow"));
    assert_eq!(input,before);
}

#[test]
fn current_components_override_redundant_inventory_and_custom_data_is_not_counted() {
    let contents=list(vec![c(vec![("slot",Value::Int(0)),("item",c(vec![("id",text("minecraft:stone")),("count",Value::Int(32)),("components",c(vec![("minecraft:max_stack_size",Value::Int(16))]))]))])]);
    let outer=c(vec![("id",text("minecraft:shulker_box")),("count",Value::Int(1)),("components",c(vec![
        ("minecraft:container",contents),("minecraft:block_entity_data",c(vec![("Items",list(vec![item("minecraft:stone",32)]))])),
        ("minecraft:custom_data",c(vec![("Items",list(vec![item("minecraft:stone",64)]))])),
    ]))]);
    let rows=count(&root(vec![c(vec![("Items",list(vec![outer]))])],vec![]),4189).unwrap();
    let stone=rows.iter().find(|r|r.name=="minecraft:stone").unwrap();
    assert_eq!(stone.count,32);assert!((stone.boxes.unwrap()-32.0/432.0).abs()<1e-10);
}

#[test]
fn bundles_defaults_unknown_capacity_and_invalid_counts_are_explicit() {
    let bundle=c(vec![("id",text("minecraft:bundle")),("components",c(vec![("minecraft:bundle_contents",list(vec![c(vec![("id",text("mod:filter"))])]))]))]);
    let rows=count(&root(vec![c(vec![("Items",list(vec![bundle]))])],vec![]),4189).unwrap();
    let unknown=rows.iter().find(|r|r.name=="mod:filter").unwrap();assert_eq!(unknown.count,1);assert_eq!(unknown.boxes,None);
    assert!(count(&root(vec![c(vec![("Items",list(vec![item("minecraft:stone",-1)]))])],vec![]),3700).unwrap_err().to_string().contains("Items[0]"));
}
