-- KONTRA's deliberately small native UI host. Unknown API is an explicit error.
-- Keep the original protected call private. A caught hook error cannot reset
-- fuel: every return, including nested calls, rethrows sticky exhaustion.
local protected_call, budget_check = pcall, native_budget_check
pcall = function(fn,...)
    budget_check()
    local result = table.pack(protected_call(fn,...))
    budget_check()
    return table.unpack(result,1,result.n)
end
local function strict(name, values)
    return setmetatable(values, {__index=function(_,key)
        error("Unsupported native UI API: "..name.."."..tostring(key),2)
    end})
end
-- Legacy native_ui_util augments Lua's table library as well as exporting
-- scalar helpers. Copies retain nested identities, never inherited fields.
table.shallow_copy = function(source)
    assert(type(source)=="table","shallow_copy requires a table")
    local result={}
    for key,value in next,source do result[key]=value end
    return result
end
native_ui_util_host = strict("native_ui_util", {
    clamp=function(value,minimum,maximum)
        assert(type(value)=="number" and type(minimum)=="number" and type(maximum)=="number"
            and value==value and minimum<=maximum,"Invalid clamp range or value")
        return math.min(maximum,math.max(minimum,value))
    end,
    partial=function(fn,...)
        assert(type(fn)=="function","partial requires a function")
        local bound=table.pack(...)
        return function(...)
            local rest=table.pack(...)
            local args={}
            for index=1,bound.n do args[index]=bound[index] end
            for index=1,rest.n do args[bound.n+index]=rest[index] end
            return fn(table.unpack(args,1,bound.n+rest.n))
        end
    end,
})
local active, instances = nil, {}
local instance_count = 0
local function color(r,g,b,a)
    local channels={r,g,b}
    for index=1,3 do
        local value=channels[index]
        assert(type(value)=="number" and value%1==0 and value>=0 and value<=255,"Invalid RGB channel")
    end
    assert(type(a)=="number" and a>=0 and a<=1,"Invalid color opacity")
    return setmetatable({r=r,g=g,b=b,a=a}, {__index=strict("Color", {
        opacity=function(self,value) return color(self.r,self.g,self.b,value) end,
    })})
end
-- Named colors are optional Lua table data: missing entries remain nil.
-- Callable APIs and color instance methods retain explicit validation.
local Color=setmetatable({
    white=color(255,255,255,1),black=color(0,0,0,1),
    red=color(255,0,0,1),none=color(0,0,0,0),
}, {})
getmetatable(Color).__call=function(_,r,g,b,a)
    if g==nil and b==nil and a==nil then
        assert(type(r)=="number" and r%1==0 and r>=-2147483648 and r<=4294967295,"Invalid packed color")
        return color((r>>16)&255,(r>>8)&255,r&255,((r>>24)&255)/255)
    end
    if a~=nil then
        assert(type(a)=="number" and a%1==0 and a>=0 and a<=255,"Invalid alpha channel")
    end
    return color(r,g,b,a==nil and 1 or a/255)
end
local ui = {
    create_state=function(value)
        if not active then
            return function() return value end, function(next_value) value=next_value; native_changed() end
        end
        active.hook = active.hook + 1
        local index, states = active.hook, active.states
        if not states[index] then states[index] = {value=value} end
        local state = states[index]
        return function() return state.value end, function(next_value) state.value=next_value; native_changed() end
    end,
    create_context=function(default) return {default=default, native_context=true} end,
    use_context=function(key)
        assert(active and type(key)=="table" and key.native_context,"Native UI context requires a component")
        local values=active.context
        return function() local value=values[key];if value==nil then return key.default end;return value end
    end,
    -- Computed properties are reevaluated when a scene snapshot is requested.
    create_computed=function(compute) assert(type(compute)=="function"); return compute end,
    Color=Color,
}
for _,kind in ipairs({"ZStack","HStack","VStack","Text","Rectangle","Image"}) do ui[kind]={kind=kind} end
native_ui_host = strict("native_ui",ui)
kontakt_host = strict("kontakt", {
    connect_level_meter=function(name) return strict("level_meter",native_connect_level_meter(name)) end,
    create_tmp_midi_file=function()
        error("Unsupported native UI API: kontakt.create_tmp_midi_file",2)
    end,
    connect_parameter=function(name)
        local slot,control = native_connect(name)
        return strict("parameter", {
            value=function(_,index)
                assert(index==nil or index==0,"Native UI array connections are unavailable")
                return native_value(slot,control)
            end,
            set_value=function(_,value,index)
                assert(index==nil or index==0,"Native UI array connections are unavailable")
                assert(type(value)=="number" and value%1==0,"Integer parameter required")
                native_set(slot,control,value)
            end,
        })
    end,
})
function native_component(target,props,children)
    assert(props.children==nil,"Explicit children property is unavailable"); props.children=children
    if type(target)=="function" then
        return {kind="Component",target=target,props=props,children=children,modifiers={}}
    end
    assert(type(target)=="table" and target.kind,"Unsupported component target")
    local supported={ZStack={children=true,alignment=true},HStack={children=true,spacing=true,alignment=true},VStack={children=true,spacing=true,alignment=true},Text={children=true,text=true},Rectangle={children=true,color=true,corner_radius=true},Image={children=true,file=true,resizable=true}}
    assert(supported[target.kind],"Unsupported component")
    assert(target.kind=="ZStack" or target.kind=="HStack" or target.kind=="VStack" or #children==0,"Primitive children require a stack")
    for key in pairs(props) do assert(supported[target.kind][key],"Unsupported primitive property: "..target.kind.."."..key) end
    return {kind=target.kind,props=props,children=children,modifiers={}}
end
function native_modifier(node,name,props,args)
    local environment=name=="context" or name=="font_size" or name=="foreground_color"
    assert(name=="frame" or name=="padding" or name=="hidden" or name=="opacity" or name=="background" or name=="on_tap_gesture" or environment,"Unsupported modifier: "..name)
    if name=="background" then
        assert(next(props)==nil and #args==1 and type(args[1])=="table" and args[1].kind,"Background requires a component")
        return {kind="ZStack",props={},children={args[1],node},modifiers={background={}}}
    end
    if name=="padding" then
        assert(#args<=1 and (#args==0 or next(props)==nil),"Invalid padding arguments")
        if #args==1 then props={left=args[1],right=args[1],top=args[1],bottom=args[1]} end
        for key,value in pairs(props) do
            assert(key=="left" or key=="right" or key=="top" or key=="bottom" or key=="horizontal" or key=="vertical","Unsupported padding property")
            assert(type(value)=="number" and value>=0 and value<=4096,"Unsupported negative or oversized native UI padding")
        end
        assert(not ((props.horizontal~=nil or props.vertical~=nil) and (props.left~=nil or props.right~=nil or props.top~=nil or props.bottom~=nil)),"Mixed padding constructors")
        props={left=props.left or props.horizontal or 0,right=props.right or props.horizontal or 0,top=props.top or props.vertical or 0,bottom=props.bottom or props.vertical or 0}
        return {kind="ZStack",props={},children={node},modifiers={padding=props}}
    end
    if name=="opacity" then
        assert(next(props)==nil and #args==1,"Invalid opacity arguments")
        local value=args[1]
        assert(type(value)=="number" or type(value)=="function","Opacity requires a number")
        return {kind="ZStack",props={},children={node},modifiers={opacity={value=value}}}
    end
    if name=="hidden" then
        assert(next(props)==nil and #args<=1,"Invalid hidden arguments")
        local value=args[1]
        if value==nil then value=true end
        assert(type(value)=="boolean" or type(value)=="function","Hidden requires a boolean")
        return {kind="ZStack",props={},children={node},modifiers={hidden={value=value}}}
    end
    if name=="font_size" or name=="foreground_color" then
        assert(next(props)==nil and #args<=1,"Invalid text style modifier: "..name)
        local value=args[1]
        assert(value==nil or type(value)=="function"
            or (name=="font_size" and type(value)=="number" and value>=0 and value<=4096)
            or (name=="foreground_color" and type(value)=="table"),"Invalid text style value: "..name)
        props={value=value}
    else
        assert(#args==0,"Unsupported positional native UI modifier: "..name)
    end
    if name=="frame" then
        for key,value in pairs(props) do
            assert(((key=="width" or key=="height") and type(value)=="number" and value>=0 and value<=4096)
                or (key=="alignment" and type(value)=="string"),"Unsupported frame property")
        end
        return {kind="ZStack",props={},children={node},modifiers={frame=props}}
    end
    -- Environment modifiers compose as actual scopes. The closest scope wins,
    -- including two context keys on one component or nested overrides.
    if environment then
        if name=="context" then
            for key in pairs(props) do assert(key=="key" or key=="value","Unsupported context property") end
            assert(type(props.key)=="table" and props.key.native_context,"Invalid context key")
        end
        return {kind="ZStack",props={},children={node},modifiers={[name]=props}}
    end
    assert(node.modifiers[name]==nil,"Duplicate modifier")
    for key,value in pairs(props) do
        assert(key=="start" and type(value)=="function","Unsupported gesture property")
    end
    node.modifiers[name]=props; return node
end

-- Component functions run within their inherited context during evaluation,
-- rather than before parent modifiers can establish that context.
function native_expand(node,path,inherited)
    assert(type(node)=="table" and node.kind,"Native component must return a component")
    local context=inherited
    local scope=node.modifiers.context
    if scope then
        assert(type(scope.key)=="table" and scope.key.native_context,"Invalid context key")
        context=setmetatable({[scope.key]=scope.value},{__index=inherited})
    end
    if node.kind~="Component" then return node,context end
    local key=path..":"..native_component_key(node.target)
    local instance=instances[key]
    if not instance then
        assert(instance_count<4096,"Native UI state-instance budget exceeded")
        instance={states={}};instances[key]=instance;instance_count=instance_count+1
    end
    instance.context=context;instance.hook=0
    local previous=active;active=instance
    local child=node.target(node.props)
    active=previous
    assert(type(child)=="table" and child.kind,"Native component function returned no component")
    return {kind="ZStack",props={},children={child},modifiers=node.modifiers},context
end
