#!/usr/bin/env python3
"""Authored reference probes of v1. No native Kontakt fidelity is inferred."""
import json,pathlib,subprocess
binary='/mnt/Windows11/DEV_WORKSPACE/Toolchains/User/cargo-target/ci/examples/ksp_audit_bridge'
probes={
 'tick_us':('on note\n$out:=ms_to_ticks(500000)\nend on',[['note']],960),
 'quarter_duration':('on note\n$out:=$DURATION_QUARTER\nend on',[['note']],500000),
 'display_pan':('on note\nset_text($label,get_engine_par_disp($ENGINE_PAR_PAN,-1,-1,-1))\nend on',[['note']],None),
 'transport_listener':('on listener\ninc($out)\nend on',[['transport',True,120]],1),
 'stop_wait':('on note\nwait(1000000)\n$out:=1\nend on\non controller\nstop_wait($id,0)\nend on',[['note'],['controller'],['process',1]],1),
 'wait_time':('on note\nwait(1000)\n$out:=1\nend on',[['note'],['process',48],['process',1]],1),
 'global_ui_compile':('on ui_controls\n$out:=1\nend on',[],None),
 'persistent_read_consumption':('',[],99),
 'early_menu_read':('',[],80),
 'invalid_menu_index':('',[],20),
}
p=subprocess.Popen([binary],stdin=subprocess.PIPE,stdout=subprocess.PIPE,text=True)
for name,(body,actions,expected) in probes.items():
    init='on init\ndeclare ui_value_edit $out(0,1000000,1)\ndeclare $id\ndeclare ui_label $label(1,1)\nset_text($label,"old")\n'
    if name=='transport_listener': init+='set_listener($NI_SIGNAL_TRANSP_START,1)\n'
    if name=='stop_wait':body=body.replace('wait(1000000)','$id:=$NI_CALLBACK_ID\nwait(1000000)')
    entries=[]
    if name=='persistent_read_consumption': init+='make_persistent($out)\nread_persistent_var($out)\n$out:=99\n';entries=['$out 5']
    if name in ('early_menu_read','invalid_menu_index'):
        init+='declare ui_menu $m\nmake_persistent($m)\n'
        if name=='early_menu_read':init+='read_persistent_var($m)\n'
        init+='add_menu_item($m,"a",20)\nadd_menu_item($m,"b",80)\n$out:=$m\n'
        entries=['$m '+('1' if name=='early_menu_read' else '99')]
        if name=='invalid_menu_index':body='on persistence_changed\n$out:=$m\nend on'
    req={'source':init+'end on\n'+body,'groups':['Group 0'],'saved':entries,'probe':True,'actions':actions}
    p.stdin.write(json.dumps(req)+'\n');p.stdin.flush();res=json.loads(p.stdout.readline())
    controls=res.pop('controls');res.pop('calls')
    value=next((c['properties']['$CONTROL_PAR_VALUE'] for c in controls if c['variable']=='$out'),None)
    label=next((c['properties'].get('$CONTROL_PAR_TEXT') for c in controls if c['variable']=='$label'),None)
    res.update(name=name,value=value,label=label,expected=expected)
    print(json.dumps(res))
p.stdin.close();assert p.wait()==0
