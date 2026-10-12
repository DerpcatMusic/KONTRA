use super::*;

#[test]
fn authored_slider_recipe_preserves_pixels_pointer_ksp_and_releases_original_artwork() {
    let root = std::env::temp_dir().join(format!("kontakto-native-vector-{}",std::process::id()));
    let folder = root.join("Resources/native_ui");
    std::fs::create_dir_all(&folder).unwrap();
    let source = r#"local ui = require('native_ui')
local level = require('kontakt').connect_parameter('level')
recipe_version = 1
recipe_width = 40
return function()
    local handle = __node(ui.Rectangle,function() return {color=ui.Color(1,1,1,1)} end)
        :frame({width=4,height=20}):offset({x=36*level:value(),y=0})
    local shape = __node(ui.Rectangle,function() return {color=ui.Color(0,0,0,1)} end)
    return __node(ui.Image,function() return {file='track.png'} end)
        :vector_recipe({version=recipe_version,role='slider',width=recipe_width,height=20,content=shape})
        :overlay({handle,alignment='top_left'})
        :on_tap_gesture({complete=function(event)
            level:set_value(event.position.x/event.frame.width)
        end})
end"#;
    std::fs::write(folder.join("root.nui"),source).unwrap();
    let mut png = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut png,40,20);
        encoder.set_color(png::ColorType::Rgba);
        encoder.write_header().unwrap().write_image_data(&[0,0,0,255].repeat(40*20)).unwrap();
    }
    std::fs::write(folder.join("track.png"),png).unwrap();
    let path = root.join("Patch.nki");
    let full = Arc::new(Package::load(&path).unwrap());
    let pixels_owner = Arc::downgrade(&full.images["track.png"]);
    let compressed_owner = Arc::downgrade(&full.members["track.png"]);
    let reduced = Arc::new(full.retain_visuals(&[],&[]));
    let mut engine = crate::ksp::LogEngine::default();
    let (mut runtime,errors) = crate::ksp::Runtime::with_scripts(&["on init\nexpose_controls\ndeclare ui_slider $level(0,100)\n$level := 25\ndeclare $hits\nmake_persistent($hits)\nend on\non ui_control($level)\ninc($hits)\nend on"],&mut engine,0,Vec::new());
    assert!(errors.iter().all(Option::is_none),"{errors:?}");
    let session = Session::new(full.clone(),"root",runtime.exposed_controls().into()).unwrap();
    let graph = session.render().unwrap();
    let id = format!("nui-0-{}",string(&graph,"path"));
    let mut demand = Vec::new();
    image_demand(&graph,false,&full,false,&mut demand).unwrap();
    assert_eq!(demand,vec!["track.png"]);
    demand.clear();
    image_demand(&graph,false,&reduced,true,&mut demand).unwrap();
    assert!(demand.is_empty());
    let size = Size::new(80.,40.);
    let mut drafts = HashMap::new();
    let mut original_ui = super::super::theme::ui();
    let original = draw(&mut original_ui,&graph,&full,&session,0,1.,Style::default(),&mut drafts).unwrap();
    original_ui.frame(original,Some(size),Input::default(),1./60.).unwrap();
    let frame = original_ui.scene().unwrap().surface(&id).unwrap().frame;
    let original_pixels = super::super::tests::pixels(&original_ui,80,40);
    let mut ui = super::super::theme::ui();
    let vector = draw(&mut ui,&graph,&reduced,&session,0,1.,Style { vector:true,..Default::default() },&mut drafts).unwrap();
    ui.frame(vector,Some(size),Input::default(),1./60.).unwrap();
    assert_eq!(ui.scene().unwrap().surface(&id).unwrap().frame,frame);
    assert_eq!(super::super::tests::pixels(&ui,80,40),original_pixels,
        "the explicit shape recipe replaces only the authored track pixels");
    drop(original_ui);
    drop(full);
    assert!(pixels_owner.upgrade().is_none(),"vector scene and Lua session retain no Original pixels");
    assert!(compressed_owner.upgrade().is_none(),"vector session retains no Original compressed artwork");

    let mut hovered = HashSet::new();
    for down in [false,true,false] {
        let el = draw(&mut ui,&graph,&reduced,&session,0,1.,Style { vector:true,..Default::default() },&mut drafts).unwrap();
        let input = Input { pointer: PointerInput {
            pos: Some(Point::new(frame.x+frame.size.width*0.75,frame.y+frame.size.height*0.5)),
            buttons: if down { Buttons::PRIMARY } else { Buttons::default() },
            ..Default::default()
        },..Default::default() };
        ui.frame(el,Some(size),input,1./60.).unwrap();
        events(&mut ui,&graph,&session,0,1.,&mut hovered).unwrap();
    }
    let edits = session.take_edits();
    assert_eq!(edits.len(),1);
    assert_eq!((edits[0].slot,edits[0].control,edits[0].value),(0,0,75.));
    runtime.ui_control(&mut engine,edits[0].slot,edits[0].control,edits[0].value as i32);
    assert_eq!(runtime.interface(0).controls[0].properties["$CONTROL_PAR_VALUE"],crate::ksp::Value::Int(75));
    assert_eq!(runtime.persistence()[0]["$hits"],crate::ksp::Value::Int(1));
    session.update(runtime.exposed_controls().into());
    let updated = session.render().unwrap();
    let el = draw(&mut ui,&updated,&reduced,&session,0,1.,Style { vector:true,..Default::default() },&mut drafts).unwrap();
    ui.frame(el,Some(size),Input::default(),1./60.).unwrap();
    assert_eq!(ui.scene().unwrap().surface(&id).unwrap().frame,frame);
    let changed_pixels = super::super::tests::pixels(&ui,80,40);
    assert_ne!(changed_pixels,original_pixels,"the original value-driven handle still moves");
    assert!(session.unavailable_controls().is_empty());

    // Unknown recipes use the very same Original demand and drawing decision.
    for (field,value) in [("recipe_version",2),("recipe_width",41)] {
        session.lua().globals().set("recipe_version",1).unwrap();
        session.lua().globals().set("recipe_width",40).unwrap();
        session.lua().globals().set(field,value).unwrap();
        let fallback = session.render().unwrap();
        demand.clear();
        image_demand(&fallback,false,&reduced,true,&mut demand).unwrap();
        assert_eq!(demand,vec!["track.png"]);
        let mut restored = reduced.retain_visuals(&[],&[]);
        restored.merge_visuals(Package::load_visuals(&path,&demand,&[]).unwrap());
        let restored = Arc::new(restored);
        let el = draw(&mut ui,&fallback,&restored,&session,0,1.,Style { vector:true,..Default::default() },&mut drafts).unwrap();
        ui.frame(el,Some(size),Input::default(),1./60.).unwrap();
        assert_eq!(ui.scene().unwrap().surface(&id).unwrap().frame,frame);
        assert_eq!(super::super::tests::pixels(&ui,80,40),changed_pixels);
    }
    session.lua().globals().set("recipe_version",1).unwrap();
    session.lua().globals().set("recipe_width",40).unwrap();
    // Structural errors in a recipe must restore Original demand and drawing,
    // rather than retiring pixels and then failing the whole visible UI.
    for malformed in ["props","children","paint"] {
        let fallback = session.render().unwrap();
        let recipe = tables(&fallback,"modifiers").unwrap().into_iter()
            .find(|modifier| string(modifier,"name")=="vector_recipe").unwrap();
        let content: Table = recipe.get::<Table>("value").unwrap().get("content").unwrap();
        if malformed == "paint" {
            content.set("kind","Canvas").unwrap();
        } else {
            content.set(malformed,Value::Nil).unwrap();
        }
        demand.clear();
        image_demand(&fallback,false,&reduced,true,&mut demand).unwrap();
        assert_eq!(demand,vec!["track.png"]);
        let mut restored = reduced.retain_visuals(&[],&[]);
        restored.merge_visuals(Package::load_visuals(&path,&demand,&[]).unwrap());
        let restored = Arc::new(restored);
        let el = draw(&mut ui,&fallback,&restored,&session,0,1.,Style { vector:true,..Default::default() },&mut drafts).unwrap();
        ui.frame(el,Some(size),Input::default(),1./60.).unwrap();
        assert_eq!(super::super::tests::pixels(&ui,80,40),changed_pixels);
    }
    assert!(runtime.diagnostics().is_empty());
    std::fs::remove_dir_all(root).unwrap();
}
