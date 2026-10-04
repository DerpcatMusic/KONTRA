-- NativeUI component graph. The host traverses this graph and renders its primitives.
local ui = {}
local methods = {}
local node_mt = {__index=methods}
local state_slots, current_path, hook_index, current_context = {}, 'root', 0, {}
local function bind(fn,path,context)
    return function(...)
        local old_path,old_index,old_context=current_path,hook_index,current_context
        current_path,hook_index,current_context=path,0,context
        local result=table.pack(pcall(fn,...))
        current_path,hook_index,current_context=old_path,old_index,old_context
        if not result[1] then error(result[2],0) end
        return table.unpack(result,2,result.n)
    end
end
local function node(component, properties)
    return setmetatable({component=component, properties=properties, modifiers={}},node_mt)
end
_G.__node = node
-- A dynamic expression must retain Lua's nil/false truthiness for `and/or`
-- fallbacks. Copy returned nodes so modifiers do not mutate reusable children.
_G.__dynamic = function(factory)
    local value=factory()
    if type(value)=='table' and value.component then
        local copy=node(value.component,value.properties)
        for i,m in ipairs(value.modifiers) do copy.modifiers[i]=m end
        return copy
    end
    return value
end
for _, name in ipairs({'position','frame','offset','align','padding','hidden','opacity','background','overlay','foreground_color','font_size','rotation','disabled','bold','multiline_text_alignment','line_limit','context','on_hover_gesture','on_tap_gesture','on_drag_gesture','on_drag','on_drop','popover'}) do
    methods[name]=function(self,value)
        self.modifiers[#self.modifiers+1]={name=name,value=value}
        return self
    end
end
for _, name in ipairs({'ZStack','HStack','VStack','Spacer','Text','TextInput','Rectangle','Image','Ring','Canvas','ForEach'}) do ui[name]=name end
function ui.create_state(default)
    hook_index=hook_index+1
    local key=current_path..':'..hook_index
    local slot=state_slots[key]
    if not slot then slot={value=default}; state_slots[key]=slot end
    return function() return slot.value end,function(value) slot.value=value end
end
function ui.create_computed(fn) return bind(fn,current_path,current_context) end
function ui.create_effect(fn) fn() end
function ui.create_context() return {} end
function ui.use_context(key)
    local value=current_context[key]
    return function() return value end
end
function ui.is_enabled() local enabled=current_context.enabled ~= false; return function() return enabled end end
function ui.resign_focus() _G.__resign_focus=true end
ui.no_style={}
local color_methods={}
function color_methods:opacity(value) return setmetatable({r=self.r,g=self.g,b=self.b,a=self.a*value},{__index=color_methods}) end
ui.Color=setmetatable({}, {__call=function(_,r,g,b,a)
    if g==nil then local n=r or 0; a=math.floor(n/0x1000000)%256/255; r=math.floor(n/0x10000)%256; g=math.floor(n/0x100)%256; b=n%256 end
    return setmetatable({r=r/255,g=g/255,b=b/255,a=a or 1},{__index=color_methods})
end})
ui.Color.white=ui.Color(255,255,255); ui.Color.black=ui.Color(0,0,0); ui.Color.red=ui.Color(255,0,0); ui.Color.blue=ui.Color(0,0,255); ui.Color.transparent=ui.Color(0,0,0,0)
table.shallow_copy=function(t) local out={}; for k,v in pairs(t) do out[k]=v end; return out end
local resolve
local function append(children,child)
    if not child then return end
    if child.kind=='Group' and #child.modifiers==0 then
        for _,nested in ipairs(child.children) do append(children,nested) end
    else children[#children+1]=child end
end
resolve=function(element,path,context,depth)
    if element==nil or element==false then return nil end
    if depth>192 then error('NativeUI component depth exceeded') end
    if type(element)~='table' or not element.component then error('Invalid NativeUI element at '..path) end
    local old_path,old_index,old_context=current_path,hook_index,current_context
    current_path,hook_index,current_context=path,0,context
    local child_context=table.shallow_copy(context)
    local props=element.properties()
    -- Conditional children leave holes in Lua's numeric table keys. ipairs
    -- stops at the first hole and would drop every following sibling.
    props.children={}
    local child_keys={}
    for key,child in pairs(props) do
        if type(key)=='number' and key>=1 and key%1==0 then
            props.children[key]=child
            child_keys[#child_keys+1]=key
        end
    end
    table.sort(child_keys)
    for _,m in ipairs(element.modifiers) do
        if m.name=='context' then child_context[m.value.key]=m.value.value end
        if m.name=='disabled' and m.value then child_context.enabled=false end
    end
    local out
    if type(element.component)=='function' then
        out=resolve(element.component(props),path..'/component',child_context,depth+1)
        if out and #element.modifiers>0 then
            out={kind='Group',props={},children={out},modifiers={},path=path}
        end
    else
        out={kind=element.component,props=props,children={},modifiers={},path=path}
        for _,key in ipairs({'paint','on_accept','on_change'}) do
            if type(props[key])=='function' then props[key]=bind(props[key],path,child_context) end
        end
        if out.kind=='ForEach' then
            out.kind='Group'
            for i,item in ipairs(props.data or {}) do
                local index=i
                local child=resolve(props.content(item,function() return index end),path..'/'..i,child_context,depth+1)
                append(out.children,child)
            end
        else
            for _,i in ipairs(child_keys) do
                local result=resolve(props.children[i],path..'/'..i,child_context,depth+1)
                append(out.children,result)
            end
        end
    end
    if out then
        for index,m in ipairs(element.modifiers) do
            if m.name=='background' or m.name=='overlay' then
                local value=resolve(m.value.component and m.value or m.value[1],path..'/'..m.name..'/'..index,child_context,depth+1)
                if value then
                    out.modifiers[#out.modifiers+1]={name=m.name,value=value,alignment=m.value.alignment}
                end
            elseif m.name=='popover' then
                if m.value.visible then
                    local value=table.shallow_copy(m.value)
                    value.content=resolve(value.content,path..'/popover/'..index,child_context,depth+1)
                    out.modifiers[#out.modifiers+1]={name=m.name,value=value}
                end
            elseif m.name=='on_tap_gesture' or m.name=='on_drag_gesture' or m.name=='on_hover_gesture' then
                local value=table.shallow_copy(m.value)
                for key,fn in pairs(value) do if type(fn)=='function' then value[key]=bind(fn,path,child_context) end end
                out.modifiers[#out.modifiers+1]={name=m.name,value=value}
            else out.modifiers[#out.modifiers+1]=m end
        end
    end
    current_path,hook_index,current_context=old_path,old_index,old_context
    return out
end
_G.__render=function(root) return resolve(node(root,function() return {} end),'root',{enabled=true},0) end
package.loaded.native_ui=ui
package.loaded.native_ui_util={clamp=function(x,a,b) return math.max(a,math.min(b,x)) end,
    partial=function(fn,...) local args=table.pack(...); return function(...) local values=table.pack(...); local all={}; for i=1,args.n do all[#all+1]=args[i] end; for i=1,values.n do all[#all+1]=values[i] end; return fn(table.unpack(all)) end end}
-- Canvas records paths in authored pixels. MUI applies the view/device scale.
_G.__painter=function()
    local commands,path={},{}
    local painter={line_width=1}
    function painter:move_to(x,y) path[#path+1]={'move',x,y} end
    function painter:line_to(x,y) path[#path+1]={'line',x,y} end
    function painter:cubic_to(a,b,c,d,x,y) path[#path+1]={'cubic',a,b,c,d,x,y} end
    function painter:close_path() path[#path+1]={'close'} end
    function painter:draw_path()
        commands[#commands+1]={path=path,fill=self.fill_style,stroke=self.stroke_style,width=self.line_width,dash=self.dash_pattern}
        path={}
    end
    function painter:draw_line(x,y,a,b) self:move_to(x,y);self:line_to(a,b);self:draw_path() end
    function painter:draw_circle(x,y,r)
        for i=0,40 do local a=i*math.pi*2/40;local px,py=x+r*math.cos(a),y+r*math.sin(a);if i==0 then self:move_to(px,py) else self:line_to(px,py) end end
        self:close_path();self:draw_path()
    end
    return commands,painter
end
