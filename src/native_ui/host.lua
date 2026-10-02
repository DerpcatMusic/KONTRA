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
local active, instances = nil, {}
local instance_count = 0
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
    Color=function(r,g,b)
        for _,v in ipairs({r,g,b}) do assert(type(v)=="number" and v>=0 and v<=255,"Invalid color") end
        return {r=r,g=g,b=b}
    end,
}
for _,kind in ipairs({"ZStack","Text","Rectangle"}) do ui[kind]={kind=kind} end
native_ui_host = strict("native_ui",ui)
kontakt_host = strict("kontakt", {
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
    local supported={ZStack={children=true},Text={children=true,text=true},Rectangle={children=true,color=true}}
    assert(supported[target.kind],"Unsupported component")
    assert(target.kind=="ZStack" or #children==0,"Primitive children require ZStack")
    for key in pairs(props) do assert(supported[target.kind][key],"Unsupported primitive property: "..target.kind.."."..key) end
    return {kind=target.kind,props=props,children=children,modifiers={}}
end
function native_modifier(node,name,props,args)
    assert(name=="frame" or name=="on_tap_gesture" or name=="context","Unsupported modifier: "..name)
    assert(#args==0,"Unsupported positional native UI modifier: "..name)
    assert(node.modifiers[name]==nil,"Duplicate modifier")
    for key,value in pairs(props) do
        if name=="frame" then
            assert((key=="width" or key=="height") and type(value)=="number" and value>=0 and value<=4096,"Unsupported frame property")
        elseif name=="context" then assert(key=="key" or key=="value","Unsupported context property")
        else assert(key=="start" and type(value)=="function","Unsupported gesture property") end
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
