-- NativeUI component graph. The host traverses this graph and renders its primitives.
local ui = {}
local methods = {}
local node_mt = {__index=methods}
_G.__rev=0 -- bumped by every state write: the host's render-cache key
local state_slots, current_path, hook_index, current_context = {}, 'root', 0, {}
local track, mark
-- Restore the caller's hook scope, then pass pcall's results through
-- without packing them into a table.
local function finish(old_path,old_index,old_context,ok,...)
    current_path,hook_index,current_context=old_path,old_index,old_context
    if not ok then error((...),0) end
    return ...
end
local function bind(fn,path,context)
    return function(...)
        local old_path,old_index,old_context=current_path,hook_index,current_context
        current_path,hook_index,current_context=path,0,context
        return finish(old_path,old_index,old_context,pcall(fn,...))
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
for _, name in ipairs({'position','frame','offset','align','padding','hidden','opacity','background','overlay','foreground_color','font_size','font','font_family','rotation','disabled','bold','multiline_text_alignment','line_limit','context','on_hover_gesture','on_tap_gesture','on_drag_gesture','on_drag','on_drop','popover'}) do
    methods[name]=function(self,value)
        if self.resolved then
            -- The memo holds this element's output: never change it behind the
            -- memo's back. The modifier applies to a copy, as in value-typed views.
            local copy=node(self.component,self.properties)
            table.move(self.modifiers,1,#self.modifiers,1,copy.modifiers)
            self=copy
        end
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
    return function() track(slot); return slot.value end,function(value)
        if slot.value==value and type(value)~='table' then return end
        slot.value=value; _G.__rev=_G.__rev+1; mark(slot)
    end
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
local native_nodes=0
-- Component memo. Each resolved element keeps the values it read (bridge
-- controls, meters, state slots) and what it produced. A later render reuses
-- the output of every element whose reads and inputs are unchanged, so a drag
-- or a meter tick only re-resolves the components that read what moved.
-- Reads are tracked, never guessed: state getters and the host's
-- parameter/meter calls record into the element whose body is running.
local memo, readers = {}, {}
_G.__take_reads=function() end -- the host replaces it with the bridge's read log
local NO_TARGETS={}
local reading -- the element whose body or props thunk is running
track=function(key)
    local e=reading
    if e and not e.deps[key] then
        e.deps[key]=true
        local set=readers[key]
        if not set then set={}; readers[key]=set end
        set[e]=true
    end
end
mark=function(key)
    local set=readers[key]
    if not set then return end
    for e in pairs(set) do
        e.own_dirty=true
        local p=e.parent
        while p and not p.sub_dirty do p.sub_dirty=true; p=p.parent end
    end
end
local function drop(e)
    for key in pairs(e.deps) do
        local set=readers[key]
        if set then set[e]=nil end
    end
    for _,k in ipairs(e.kids) do drop(k) end
    if memo[e.path]==e then memo[e.path]=nil end
end
local function same_context(a,b)
    if a==b then return true end
    for k,v in pairs(a) do if b[k]~=v then return false end end
    for k in pairs(b) do if a[k]==nil then return false end end
    return true
end
-- Run the element's props thunk and body, recording what they read, and list
-- the child elements to resolve. Nothing here depends on the children's output.
local function plan(e)
    local element,path,context=e.element,e.path,e.context
    local old_path,old_index,old_context,old_reading=current_path,hook_index,current_context,reading
    current_path,hook_index,current_context=path,0,context
    local child_context=context
    for _,m in ipairs(element.modifiers) do
        if m.name=='context' or (m.name=='disabled' and m.value) then
            if child_context==context then child_context=table.shallow_copy(context) end
            if m.name=='context' then child_context[m.value.key]=m.value.value
            else child_context.enabled=false end
        end
    end
    current_context=child_context
    e.child_context=child_context
    e.deps={}
    reading=e
    local props=element.properties()
    -- Conditional children leave holes in Lua's numeric table keys. ipairs
    -- stops at the first hole and would drop every following sibling.
    local children={}
    props.children=children
    local child_keys={}
    local is_function=type(element.component)=='function'
    e.is_function=is_function
    -- Retain resolved children, without also retaining their source factories.
    local resolved_props={}
    for key,value in pairs(props) do
        if type(key)=='number' and key>=1 and key%1==0 then
            children[key]=value
            child_keys[#child_keys+1]=key
        elseif key~='children' and not is_function then
            resolved_props[key]=value
        end
    end
    table.sort(child_keys)
    local items={}
    e.items=items
    e.props=resolved_props
    e.own_live=false
    e.own_texts=nil
    if is_function then
        items[1]={element.component(props),path..'/component'}
        e.body=1
    else
        if element.component=='TextInput' then e.own_texts={path} end
        local paint,accept,change=props.paint,props.on_accept,props.on_change
        if type(paint)=='function' then resolved_props.paint=bind(paint,path,child_context) end
        if type(accept)=='function' then resolved_props.on_accept=bind(accept,path,child_context) end
        if type(change)=='function' then resolved_props.on_change=bind(change,path,child_context) end
        if element.component=='ForEach' then
            for i,item in ipairs(props.data or {}) do
                local index=i
                items[#items+1]={props.content(item,function() return index end),path..'/'..i}
            end
        else
            for _,i in ipairs(child_keys) do items[#items+1]={children[i],path..'/'..i} end
        end
        e.body=#items
    end
    for index,m in ipairs(element.modifiers) do
        if m.name=='background' or m.name=='overlay' then
            items[#items+1]={m.value.component and m.value or m.value[1],path..'/'..m.name..'/'..index}
        elseif m.name=='popover' and m.value.visible then
            e.own_live=true
            items[#items+1]={m.value.content,path..'/popover/'..index}
        end
    end
    reading=old_reading
    current_path,hook_index,current_context=old_path,old_index,old_context
    -- Reads the host made on the body's behalf (bridge controls and meters).
    local ids=_G.__take_reads()
    if ids then for i=1,#ids do e.deps[ids[i]]=true end end
    for key in pairs(e.deps) do
        local set=readers[key]
        if not set then set={}; readers[key]=set end
        set[e]=true
    end
end
-- Resolve the planned children and build this element's output.
local function assemble(e,depth)
    local element,path,child_context=e.element,e.path,e.child_context
    local old_kids=e.kids
    e.sub_dirty=false
    local kids,items,results={},e.items,{}
    local live,texts=e.own_live,e.own_texts
    -- Handler-bearing nodes in event order: this node, then background and
    -- overlay, then children. A popover anywhere below needs the full walk.
    local child_targets,modifier_targets,all_targets={},{},true
    local function visit(i)
        local item=items[i]
        local r,k=resolve(item[1],item[2],child_context,depth+1,e)
        results[i]=r
        if k then
            kids[#kids+1]=k
            local t=k.targets
            if not t then all_targets=false
            elseif #t>0 then
                local into=i<=e.body and child_targets or modifier_targets
                into[#into+1]=t
            end
            live=live or k.live
            if k.texts then
                if not texts then texts=k.texts
                else
                    local merged=table.move(texts,1,#texts,1,{})
                    table.move(k.texts,1,#k.texts,#merged+1,merged)
                    texts=merged
                end
            end
        end
    end
    for i=1,e.body do visit(i) end
    local out
    if e.is_function then
        out=results[1]
        if out and #element.modifiers>0 then
            out={kind='Group',props={},children={out},modifiers={},path=path}
        end
    else
        out={kind=element.component,props=e.props,children={},modifiers={},path=path}
        if out.kind=='ForEach' then out.kind='Group' end
        for i=1,e.body do append(out.children,results[i]) end
    end
    local own=false
    if out then
        local at=e.body
        for index,m in ipairs(element.modifiers) do
            if m.name=='background' or m.name=='overlay' then
                at=at+1; visit(at)
                local value=results[at]
                if value then
                    out.modifiers[#out.modifiers+1]={name=m.name,value=value,alignment=m.value.alignment}
                end
            elseif m.name=='popover' then
                if m.value.visible then
                    all_targets=false
                    at=at+1; visit(at)
                    local value=table.shallow_copy(m.value)
                    value.content=results[at]
                    out.modifiers[#out.modifiers+1]={name=m.name,value=value}
                end
            elseif m.name=='on_tap_gesture' or m.name=='on_drag_gesture' or m.name=='on_hover_gesture' then
                local value=table.shallow_copy(m.value)
                for key,fn in pairs(value) do if type(fn)=='function' then value[key]=bind(fn,path,child_context) end end
                out.modifiers[#out.modifiers+1]={name=m.name,value=value}
                own=true
            else out.modifiers[#out.modifiers+1]=m end
        end
    end
    local targets=false
    if all_targets then
        local parts={}
        if own then parts[1]={out} end
        for _,t in ipairs(modifier_targets) do parts[#parts+1]=t end
        for _,t in ipairs(child_targets) do parts[#parts+1]=t end
        if #parts==0 then targets=NO_TARGETS
        elseif #parts==1 then targets=parts[1]
        else
            targets={}
            for _,t in ipairs(parts) do table.move(t,1,#t,#targets+1,targets) end
        end
    end
    e.out,e.kids,e.live,e.texts,e.targets=out,kids,live,texts,targets
    local kept={}
    for _,k in ipairs(kids) do kept[k]=true end
    for _,k in ipairs(old_kids) do if not kept[k] then drop(k) end end
end
resolve=function(element,path,context,depth,parent)
    if element==nil or element==false then return nil end
    if depth>192 then error('NativeUI component depth exceeded') end
    native_nodes=native_nodes+1
    _G.__native_nodes=native_nodes
    if native_nodes>16384 then error('NativeUI component budget exceeded') end
    if type(element)~='table' or not element.component then error('Invalid NativeUI element at '..path) end
    local e=memo[path]
    if e and e.element==element and not e.own_dirty and same_context(e.context,context) then
        e.parent=parent
        if e.sub_dirty then assemble(e,depth) end
        return e.out,e
    end
    if e then drop(e) end
    element.resolved=true
    e={element=element,path=path,context=context,parent=parent,kids={}}
    memo[path]=e
    plan(e)
    assemble(e,depth)
    return e.out,e
end
local root_element, root_function
_G.__render=function(root,changed)
    native_nodes=0; _G.__native_nodes=0; reading=nil
    _G.__take_reads()
    if changed then for i=1,#changed do mark(changed[i]) end end
    if root_function~=root then root_function=root; root_element=node(root,function() return {} end) end
    local out,e=resolve(root_element,'root',{enabled=true},0,nil)
    _G.__live=e and e.live or false
    _G.__texts=e and e.texts or {}
    if e then _G.__targets=e.targets else _G.__targets=NO_TARGETS end
    return out
end
-- Forget every memo entry: used when a render failed half way.
_G.__reset=function() memo,readers={}, {}; reading=nil; root_function=nil end
-- Audit: resolve from scratch beside the memo and report the first place the
-- memoized graph differs from it. Functions compare by type only, and places
-- where two fresh resolves already disagree (a body that counts its own runs)
-- are not the memo's doing and are skipped.
_G.__verify=function(root,current)
    local saved_memo,saved_readers,saved_reading=memo,readers,reading
    local function fresh()
        memo,readers,reading={}, {}, nil
        native_nodes=0
        return (resolve(node(root,function() return {} end),'root',{enabled=true},0,nil))
    end
    local first=fresh()
    local others={}
    for i=1,8 do others[i]=fresh() end
    memo,readers,reading=saved_memo,saved_readers,saved_reading
    local function diffs(a,b,where,out,seen)
        if type(a)=='table' and type(a.path)=='string' and a.kind then where=where..'@'..a.path end
        if type(a)~=type(b) then out[#out+1]=where; return end
        if type(a)=='function' then return end
        if type(a)~='table' then
            if a~=b and not (a~=a and b~=b) then out[#out+1]=where; out[where]=tostring(a)..' vs '..tostring(b) end
            return
        end
        if seen[a] then return end
        seen[a]=true
        for k,v in pairs(a) do
            if type(k)~='string' or k:sub(1,2)~='__' then diffs(v,b[k],where..'.'..tostring(k),out,seen) end
        end
        for k in pairs(b) do
            if a[k]==nil and (type(k)~='string' or k:sub(1,2)~='__') then out[#out+1]=where..'.'..tostring(k) end
        end
    end
    local unstable={}
    local list={}
    for _,other in ipairs(others) do
        list={}
        diffs(first,other,'root',list,{})
        for _,w in ipairs(list) do unstable[w]=true end
    end
    list={}
    diffs(current,first,'root',list,{})
    for _,w in ipairs(list) do if not unstable[w] then return w..' ('..tostring(list[w])..')' end end
    return nil
end
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
