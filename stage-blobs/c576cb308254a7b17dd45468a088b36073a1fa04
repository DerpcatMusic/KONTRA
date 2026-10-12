use super::*;

#[test]
fn hidden_missing_images_preserve_visible_lua_control_and_measured_layout() {
    let source = r#"local ui = require('native_ui')
hidden_image = true
taps = 0
return function()
    local visible = __node(ui.Rectangle, function() return {} end)
        :frame({width=40,height=20})
        :on_tap_gesture({complete=function() taps=taps+1 end})
    local known = __node(ui.Image, function() return {file='known.png'} end)
        :on_tap_gesture({complete=function() end})
    local missing = __node(ui.Image, function() return {file='missing.png'} end)
        :frame({width=17,height=23})
    local hidden = __node(ui.HStack, function() return {known,missing} end)
        :hidden(hidden_image)
    return __node(ui.HStack, function() return {visible,hidden} end)
end"#;
    let image = Arc::new(moose::mui::mui::scene::Image::rgba(32,24,vec![255;32*24*4]).unwrap());
    let package = Arc::new(Package {
        members: [("root.nui".into(),Arc::<[u8]>::from(source.as_bytes()))].into_iter().collect(),
        images: [("known.png".into(),image.clone())].into_iter().collect(),
        fonts: Default::default(),
        image_metadata: [("known.png".into(),[32,24])].into_iter().collect(),
    });
    let session = Session::new(package.clone(),"root",Arc::from([])).unwrap();
    let graph = session.render().unwrap();
    let mut demand = Vec::new();
    image_demand(&graph,false,&package,false,&mut demand).unwrap();
    assert!(demand.is_empty());
    let children = tables(&graph,"children").unwrap();
    let visible_id = format!("nui-0-{}",string(&children[0],"path"));
    let known = tables(&children[1],"children").unwrap().remove(0);
    let known_id = format!("nui-0-{}",string(&known,"path"));
    let mut ui = super::super::theme::ui();
    let mut drafts = HashMap::new();
    let el = draw(&mut ui,&graph,&package,&session,0,1.,Style::default(),&mut drafts).unwrap();
    ui.frame(el,Some(Size::new(120.,50.)),Input::default(),1./60.).unwrap();
    let frame = ui.scene().unwrap().surface(&visible_id).unwrap().frame;
    assert_eq!(frame.size,Size::new(40.,20.));
    assert_eq!(ui.scene().unwrap().surface(&known_id).unwrap().frame.size,Size::new(32.,24.));
    assert_eq!(Arc::strong_count(&image),2,"hidden drawing must not lease decoded pixels");
    let mut hovered = HashSet::new();
    for down in [false,true,false] {
        let el = draw(&mut ui,&graph,&package,&session,0,1.,Style::default(),&mut drafts).unwrap();
        let input = Input { pointer: PointerInput {
            pos: Some(Point::new(frame.x+20.,frame.y+10.)),
            buttons: if down { Buttons::PRIMARY } else { Buttons::default() },
            ..Default::default()
        },..Default::default() };
        ui.frame(el,Some(Size::new(120.,50.)),input,1./60.).unwrap();
        events(&mut ui,&graph,&session,0,1.,&mut hovered).unwrap();
    }
    assert_eq!(session.lua().globals().get::<i64>("taps").unwrap(),1);
    session.lua().globals().set("hidden_image",false).unwrap();
    let visible_missing = session.render().unwrap();
    assert!(draw(&mut ui,&visible_missing,&package,&session,0,1.,Style::default(),&mut drafts)
        .err().unwrap().to_string().contains("Missing NativeUI image"));
}
