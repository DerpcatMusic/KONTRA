-- Developer-only scene/control protocol. No plugin or renderer invokes this.
-- lua5.4 tools/probe_native_ui_scene.lua <lowered-root> <control-registry.tsv>
local root, registry_file = assert(arg[1], "lowered resource root"), assert(arg[2], "control registry")
local controls, loaded, loading, request = {}, {}, {}, 0
for row in io.lines(registry_file) do
    local name, slot, control, value = row:match("^([^\t]+)\t(%d+)\t(%d+)\t(-?%d+)$")
    assert(name, "Invalid registry row")
    assert(not controls[name], "Registry must resolve duplicate identifiers explicitly")
    controls[name] = {slot=tonumber(slot), control=tonumber(control), value=tonumber(value)}
end
local function strict(name, values)
    return setmetatable(values, {__index=function(_, key)
        error("Unsupported native UI API: " .. name .. "." .. tostring(key), 2)
    end})
end
local ui = {
    create_state=function(value)
        return function() return value end, function(next_value) value=next_value end
    end,
    -- Pull-based only; there is no dependency/subscription scheduler yet.
    create_computed=function(compute) return compute end,
    Color=function(r,g,b)
        for _,v in ipairs({r,g,b}) do assert(v >= 0 and v <= 255, "Invalid color channel") end
        return {r=r,g=g,b=b}
    end,
}
for _,kind in ipairs({"ZStack","Text","Rectangle"}) do ui[kind]={kind=kind} end
loaded.native_ui = strict("native_ui", ui)
loaded.kontakt = strict("kontakt", {
    connect_parameter=function(name)
        local address = assert(controls[name], "Unknown exposed control: " .. tostring(name))
        return strict("parameter", {
            value=function(_,index)
                assert(index == nil or index == 0, "Array connections are not implemented")
                return address.value
            end,
            set_value=function(_,value,index)
                assert(index == nil or index == 0, "Array connections are not implemented")
                assert(type(value)=="number" and value%1==0, "Integer control value required")
                request=request+1
                io.write(string.format("REQUEST\t%d\t%d\t%d\t%d\n",request,address.slot,address.control,value))
                io.flush()
                local serial, actual=assert(io.read("*l"), "Host reply missing")
                    :match("^APPLIED\t(%d+)\t(-?%d+)$")
                assert(tonumber(serial)==request, "Host reply identity mismatch")
                address.value=assert(tonumber(actual), "Host value missing")
            end,
        })
    end,
})
local function component(target,props,children)
    assert(props.children==nil,"Explicit children property is unsupported")
    props.children=children
    if type(target)=="function" then return target(props) end
    assert(type(target)=="table" and target.kind,"Unsupported component target")
    local supported={ZStack={children=true},Text={children=true,text=true},Rectangle={children=true,color=true}}
    for key in pairs(props) do
        assert(supported[target.kind][key], "Unsupported primitive property: " .. target.kind .. "." .. key)
    end
    return {kind=target.kind,props=props,children=children,modifiers={}}
end
local function modifier(node,name,props)
    assert(name=="frame" or name=="on_tap_gesture", "Unsupported modifier: " .. name)
    assert(node.modifiers[name]==nil,"Duplicate modifier")
    for key,value in pairs(props) do
        if name=="frame" then
            assert((key=="width" or key=="height") and type(value)=="number"
                and value>=0 and value<=4096,"Unsupported frame property")
        else assert(key=="start" and type(value)=="function","Unsupported gesture property") end
    end
    node.modifiers[name]=props
    return node
end
local environment={assert=assert,error=error,pcall=pcall,type=type,tostring=tostring,
    tonumber=tonumber,pairs=pairs,ipairs=ipairs,next=next,math=math,string=string,table=table,
    native_component=component,native_modifier=modifier}
local module_count,module_bytes=0,0
local function resource_require(name)
    assert(type(name)=="string" and name:match("^[%w_.-]+$")
        and not name:find("..",1,true) and name:sub(1,1)~=".","Invalid resource module")
    if loaded[name] then return loaded[name] end
    assert(not loading[name], "Circular native UI module: " .. name)
    local relative=name:gsub("%.","/")
    local file=io.open(root.."/"..relative..".nui","rb") or io.open(root.."/"..relative.."/init.nui","rb")
    assert(file, "Native UI module missing: " .. name)
    local source=file:read(1024*1024+1); file:close()
    module_count,module_bytes=module_count+1,module_bytes+#source
    assert(#source<=1024*1024 and module_count<=256 and module_bytes<=8*1024*1024,"Resource budget exceeded")
    local chunk,reason=load(source,"@native_ui/"..relative..".nui","t",environment)
    assert(chunk, "Native UI module syntax: " .. tostring(reason))
    loading[name]=true
    local ok,result=pcall(chunk)
    loading[name]=nil
    assert(ok,result)
    loaded[name]=result==nil and true or result
    return loaded[name]
end
environment.require=resource_require
local instructions=0
debug.sethook(function()
    instructions=instructions+1000
    assert(instructions<=100000,"Native UI instruction budget exceeded")
end,"",1000)
local scene=resource_require("main")()
assert(scene.kind=="ZStack" and scene.modifiers.frame.width==640 and scene.modifiers.frame.height==400)
local gestures={}
local function text(value)
    value=type(value)=="function" and value() or value
    return tostring(value):gsub("\t","\\t"):gsub("\n","\\n")
end
local function emit(node,path)
    io.write("NODE\t"..path.."\t"..node.kind)
    if node.props.text then io.write("\t"..text(node.props.text)) end
    io.write("\n")
    if node.modifiers.on_tap_gesture then gestures[#gestures+1]=node.modifiers.on_tap_gesture.start end
    for i,child in ipairs(node.children) do emit(child,path.."."..i) end
end
emit(scene,"0"); io.flush()
assert(#gestures==2,"Authored fixture must produce two gestures")
for _,gesture in ipairs(gestures) do gesture({kind="fixture_tap"}) end
gestures={}
emit(scene,"0")
debug.sethook()
io.write("DONE\t"..request.."\tproduction_runtime=false\trendering=false\n"); io.flush()
