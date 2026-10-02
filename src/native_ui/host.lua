-- KONTRA's deliberately small native UI host. Unknown API is an explicit error.
local function strict(name, values)
    return setmetatable(values, {__index=function(_,key)
        error("Unsupported native UI API: "..name.."."..tostring(key),2)
    end})
end
local ui = {
    create_state=function(value)
        return function() return value end, function(next_value) value=next_value; native_changed() end
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
local component_id=0
function native_component(target,props,children)
    assert(props.children==nil,"Explicit children property is unavailable"); props.children=children
    if type(target)=="function" then return target(props) end
    assert(type(target)=="table" and target.kind,"Unsupported component target")
    local supported={ZStack={children=true},Text={children=true,text=true},Rectangle={children=true,color=true}}
    assert(supported[target.kind],"Unsupported component")
    assert(target.kind=="ZStack" or #children==0,"Primitive children require ZStack")
    for key in pairs(props) do assert(supported[target.kind][key],"Unsupported primitive property: "..target.kind.."."..key) end
    assert(component_id<4096,"Native UI component count exceeds 4096")
    local id=component_id;component_id=component_id+1
    return {id=id,kind=target.kind,props=props,children=children,modifiers={}}
end
function native_modifier(node,name,props,args)
    assert(name=="frame" or name=="on_tap_gesture","Unsupported modifier: "..name)
    assert(#args==0,"Unsupported positional native UI modifier: "..name)
    assert(node.modifiers[name]==nil,"Duplicate modifier")
    for key,value in pairs(props) do
        if name=="frame" then
            assert((key=="width" or key=="height") and type(value)=="number" and value>=0 and value<=4096,"Unsupported frame property")
        else assert(key=="start" and type(value)=="function","Unsupported gesture property") end
    end
    node.modifiers[name]=props; return node
end
