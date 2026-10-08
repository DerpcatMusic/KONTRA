#!/usr/bin/env python3
"""Authored reference probes of v1. No native Kontakt fidelity is inferred."""
import json,pathlib,subprocess
binary='/mnt/Windows11/DEV_WORKSPACE/Toolchains/User/cargo-target/ci/examples/ksp_audit_bridge'
probes={
 'tick_us':('on note $out:=ms_to_ticks(500000) end on',[['note']],960),
 'quarter_duration':('on note $out:=$DURATION_QUARTER end on',[['note']],500000),
 'display_pan':('on note set_text($label,get_engine_par_disp($ENGINE_PAR_PAN,-1,-1,-1)) end on',[['note']],None),
 'transport_listener':('on listener inc($out) end on',[['transport',True,120]],1),
 'stop_wait':('on note wait(1000000) $out:=1 end on on controller stop_wait($id,0) end on',[['note'],['controller'],['process',1]],1),
 'wait_time':('on note wait(1000) $out:=1 end on',[['note'],['process',48]],1),
 'global_ui_compile':('on ui_controls $out:=1 end on',[],None),
 'persistent_read_consumption':('',[],99),
 'early_menu_read':('',[],80),
 'invalid_menu_index':('',[],20),
}
p=subprocess.Popen([binary],stdin=subprocess.PIPE,stdout=subprocess.PIPE,text=True)
for name,(body,actions,expected) in probes.items():
    init='on init declare ui_value_edit $out(0,1000000,1) declare $id declare ui_label $label(1,1) set_text($label,"old") '
    if name=='transport_listener': init+='set_listener($NI_SIGNAL_TRANSP_START,1) '
    if name=='stop_wait':body=body.replace('wait(1000000)','$id:=$NI_CALLBACK_ID wait(1000000)')
    entries=[]
    if name=='persistent_read_consumption': init+='make_persistent($out) read_persistent_var($out) $out:=99 ';entries=['$out 5']
    if name in ('early_menu_read','invalid_menu_index'):
        init+='declare ui_menu $m make_persistent($m) '
        if name=='early_menu_read':init+='read_persistent_var($m) '
        init+='add_menu_item($m,"a",20) add_menu_item($m,"b",80) $out:=$m '
        entries=['$m '+('1' if name=='early_menu_read' else '99')]
        if name=='invalid_menu_index':body='on persistence_changed $out:=$m end on'
    req={'source':init+'end on '+body,'groups':['Group 0'],'saved':entries,'probe':True,'actions':actions}
    p.stdin.write(json.dumps(req)+'\n');p.stdin.flush();res=json.loads(p.stdout.readline())
    controls=res.pop('controls');res.pop('calls')
    value=next(c for c in controls if c['variable']=='$out')['properties']['$CONTROL_PAR_VALUE']
    label=next(c for c in controls if c['variable']=='$label')['properties'].get('$CONTROL_PAR_TEXT')
    res.update(name=name,value=value,label=label,expected=expected)
    print(json.dumps(res))
p.stdin.close();assert p.wait()==0
