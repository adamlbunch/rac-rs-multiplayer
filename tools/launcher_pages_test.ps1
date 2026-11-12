# launcher_pages_test.ps1 - [MP] test only: the Settings and Mods pages of the setup window (launcher_pages.ps1), headless.
# setup.ps1 -TestNoShow -TestChoice pages [-TestShot <folder>] [-TestDpi N -TestScreen WxH] dot-sources this inside its window block.
# Everything is written into <shot folder>\pages_data (its own player\ and mods\), never into this install's. Each click is made by calling
# what the click calls, then the file the game reads is checked line by line. Shots: pages_*.png. The exit code is the number of failures.

$getState = [System.Windows.Forms.Control].GetMethod('GetState', [System.Reflection.BindingFlags]'NonPublic,Instance', $null, [Type[]]@([int]), $null)
function Own-Visible($c) { return [bool]$getState.Invoke($c, @(2)) }      # (.Visible reads false while the window is not shown)
$data = Join-Path $TestShot 'pages_data'
$src = Join-Path $TestShot 'pages_src'
foreach ($d in @($data, $src)) { if ($d.StartsWith($TestShot) -and (Test-Path -LiteralPath $d)) { Remove-Item -LiteralPath $d -Recurse -Force } }
$pl = Join-Path $data 'player'; $md = Join-Path $data 'mods'
foreach ($d in @($pl, $md, $src)) { [void][System.IO.Directory]::CreateDirectory($d) }
$utf8 = New-Object System.Text.UTF8Encoding($false)
function Put([string]$path, [string]$text) { [void][System.IO.Directory]::CreateDirectory((Split-Path -Parent $path)); [System.IO.File]::WriteAllText($path, $text, $utf8) }
function Opts { return [System.IO.File]::ReadAllText((Join-Path $pl 'pc_options.txt')) }
function Active { return [System.IO.File]::ReadAllText((Join-Path $md 'active.txt')) }
function SetJson { return [System.IO.File]::ReadAllText((Join-Path $md 'settings.json')) }
function Ctx([string]$key) { foreach ($c in $S.setCtl) { if ($c.row.key -eq $key) { return $c } }; throw "no row $key" }
function Pick([string]$key, [int]$i) { $c = Ctx $key; & $c.ctl.Tag.onPick $c $i }

# ---- fit: every visible control inside its parent, its text not clipped, no two neighbours overlapping (the whole page, nested)
function Test-Fit([string]$tag, $parent) {
    $bad = New-Object System.Collections.Generic.List[string]
    $walk = {
        param($p)
        $kids = @($p.Controls | Where-Object { Own-Visible $_ })
        foreach ($c in $kids) {
            $nm = $(if ($c.Text) { $c.Text } else { $c.GetType().Name })
            # (a scrolling area's content may be taller than the area: that is what it scrolls)
            if ($c.Right -gt $p.ClientSize.Width + 1 -or $c.Left -lt 0 -or ($c.Bottom -gt $p.ClientSize.Height + 1 -and -not $p.AutoScroll)) { $bad.Add("'$nm' outside its parent ($($c.Bounds) in $($p.ClientSize))") }
            if (($c -is [System.Windows.Forms.Label] -or $c -is [System.Windows.Forms.Button]) -and $c.Text) {
                $isBtn = ($c -is [System.Windows.Forms.Button])
                $fl = [System.Windows.Forms.TextFormatFlags]::NoPrefix; if (-not $isBtn) { $fl = $fl -bor [System.Windows.Forms.TextFormatFlags]::WordBreak }
                $txt = $(if ($isBtn -and -not $c.UseVisualStyleBackColor) { $c.Text.ToUpperInvariant() } else { $c.Text })
                $pad = $(if ($isBtn) { [int](10 * $LayoutK) } else { 0 })
                $m = [System.Windows.Forms.TextRenderer]::MeasureText($txt, $c.Font, (New-Object System.Drawing.Size(($c.ClientSize.Width - $pad), 100000)), $fl)
                if (($isBtn -and $m.Width -gt $c.ClientSize.Width - $pad) -or (-not $isBtn -and $m.Height -gt $c.ClientSize.Height)) { $bad.Add("text of '$nm' clipped (needs $($m.Width)x$($m.Height), box $($c.ClientSize.Width)x$($c.ClientSize.Height))") }
            }
            if ($c -is [System.Windows.Forms.Panel] -and $c.Controls.Count -gt 0) { & $walk $c }
        }
        for ($i = 0; $i -lt $kids.Count; $i++) { for ($j = $i + 1; $j -lt $kids.Count; $j++) {
            if ($kids[$i].Bounds.IntersectsWith($kids[$j].Bounds)) { $bad.Add("'$($kids[$i].Text)' and '$($kids[$j].Text)' overlap ($($kids[$i].Bounds) / $($kids[$j].Bounds))") }
        } }
    }
    & $walk $parent
    foreach ($b in ($bad | Select-Object -First 8)) { Write-Host "    $tag : $b" }
    Test-Check "$tag : every control fits (inside, text not clipped, no overlap) [$($bad.Count) problems]" ($bad.Count -eq 0)
}
function Test-Window([string]$tag) { Test-Check "$tag : window $($form.Width)x$($form.Height) fits the work area $($wa.Width)x$($wa.Height)" (($form.Width -le $wa.Width) -and ($form.Height -le $wa.Height)) }
function Shot-Page([string]$tag, $pg) {
    Test-Window $tag
    Test-Fit $tag $pg.panel
    if ($pg.content) { Test-Check "$tag : the page content is as wide as its scrolling area allows" ($pg.content.Width -le $pg.scroll.ClientSize.Width) }
    Save-Shot ('pages_' + $tag)
    $sc = $pg.scroll
    if ($pg.content -and $pg.content.Height -gt $sc.ClientSize.Height) {
        $sc.AutoScrollPosition = New-Object System.Drawing.Point(0, [int]($pg.content.Height / 2 - $sc.ClientSize.Height / 2)); Save-Shot ('pages_' + $tag + '_mid')
        $sc.AutoScrollPosition = New-Object System.Drawing.Point(0, $pg.content.Height); Save-Shot ('pages_' + $tag + '_end')
        $sc.AutoScrollPosition = New-Object System.Drawing.Point(0, 0)
    }
}

# ---- the data: an options file the game wrote (with a v0.4.0 Culling Off, the keyboard keys and a key from the future), and mods
Put (Join-Path $pl 'pc_options.txt') "version=1`nlang=de`nlighting=enhanced`nao=low`nculling=off`nkeys=cross:KeyK`nfps=60`nhudscale=110`nfuture_thing=7`n"
robocopy (Join-Path $root 'bundled\mods\arsenal-mastery') (Join-Path $md 'arsenal-mastery') /E /NFL /NDL /NJH /NJS /NP | Out-Null
Put (Join-Path $md 'hd-pack\mod.json') '{"format":1,"id":"hd-pack","name":"HD Pack","author":"Someone","version":"2.0","description":"Sharper textures for every planet.","kind":"textures","serials":["SCUS-97199"],"mip_names":"single"}'
Put (Join-Path $md 'hd-pack\textures\0123456789abcdef-89abcdef.png') 'x'
Put (Join-Path $md 'loose-pack\textures\0123456789abcdef-89abcdef.png') 'x'
Put (Join-Path $md 'broken-mod\omni-mod.json') '{"format":2,"id":"Broken_Mod","games":{"rac1":["pal"]}}'
Put (Join-Path $md 'other-game\omni-mod.json') '{"format":2,"id":"other-game","name":"Other Game Mod","type":"script","games":{"rac2":["pal"]}}'
$keysDemo = @'
{"format":2,"id":"keys-demo","name":"Keys Demo","version":"1.0.0","author":"tester","type":"script","games":{"rac1":["pal","ntsc"]},
 "settings":[
  {"key":"jump","name":"Jump","type":"keybind","default":"Space","group":"g","description":"The jump key."},
  {"key":"fire","name":"Fire","type":"keybind","default":"KeyF","binds":1,"group":"g"},
  {"key":"title","name":"Title text","type":"string","default":"Hello","max_length":12},
  {"key":"count","name":"Count","type":"int","default":3},
  {"key":"lives","name":"Lives","type":"int","default":3,"min":1,"max":9},
  {"key":"colour","name":"Colour","type":"choice","default":"red","choices":[{"value":"red","name":"Red"},{"value":"green","name":"Green"},{"value":"blue","name":"Blue"},{"value":"cyan","name":"Cyan"},{"value":"pink","name":"Pink"}]},
  {"key":"mode","name":"Mode","type":"choice","default":"a","choices":[{"value":"a","name":"The first way of doing it, explained at length"},{"value":"b","name":"The second way, explained at even greater length"},{"value":"c","name":"A third way"}]}
 ]}
'@
Put (Join-Path $md 'keys-demo\omni-mod.json') $keysDemo
Put (Join-Path $md 'active.txt') "# Enabled mods, one id per line, highest priority first. Written by the RAC-RS Launcher.`n# my note about the HD pack`nhd-pack`narsenal-mastery`nnot-installed-id`n"
Put (Join-Path $md 'settings.json') '{"format":1,"mods":{"arsenal-mastery":{"xp_rate":2.0,"unknown_key":5},"gone-mod":{"a":1}}}'
$S.pagesRoot = $data
Write-Host ('  layout scale {0:N2}, screen {1}x{2}, data in {3}' -f $LayoutK, $wa.Width, $wa.Height, $data)

# ---- the home view: Settings, Mods and Game folder under Play together
Show-View 'installed'
Test-Check 'home: Settings and Mods buttons shown' ((Own-Visible $UI.btnSettings) -and (Own-Visible $UI.btnMods) -and (Own-Visible $UI.btnFolder))
Test-Window 'home'; Test-Fit 'home' $pDone; Save-Shot 'pages_00_home'

# ---- Settings
$S.pageBack = 'installed'; Show-View 'settings'
Test-Check 'settings: the v0.4.0 Culling Off reads On, the options are Custom' ((Get-Opt 'culling') -eq 'on' -and (Get-GfxPresetIndex) -eq 3 -and $S.presetCtl.Tag.sel -eq 3)
Test-Check 'settings: rows show the file (lighting Enhanced, ao Low, fps 60, HUD scale 110, language Deutsch)' (((Ctx 'lighting').ctl.Tag.sel -eq 1) -and ((Ctx 'ao').ctl.Tag.sel -eq 1) -and ((Ctx 'fps').ctl.Tag.sel -eq 2) -and ((Ctx 'hudscale').vlbl.Text -eq '110%') -and ((Ctx 'lang').ctl.Tag.sel -eq 2))
Test-Check 'settings: camera speed unset shows the game''s own setting' ((Ctx 'camspeed').vlbl.Text -eq "Game's own setting" -and -not (Own-Visible (Ctx 'camspeed').lnk))
Shot-Page '10_settings' $PageView.settings
Pick 'water' 1
$t = Opts
Test-Check 'settings: Water Low is written, the other lines kept' ($t -match "(?m)^water=low$" -and $t -match "(?m)^keys=cross:KeyK$" -and $t -match "(?m)^future_thing=7$" -and $t -match "(?m)^fps=60$" -and $t -match "(?m)^culling=on$" -and $t.StartsWith("version=1`n") -and -not $t.Contains("`r"))
Test-Check 'settings: no BOM' ([System.IO.File]::ReadAllBytes((Join-Path $pl 'pc_options.txt'))[0] -ne 0xEF)
& $S.presetCtl.Tag.onPick $null 0
$t = Opts
Test-Check 'settings: preset Low sets the table (defaults left out) and Culling On' ($t -match "(?m)^draw=low$" -and $t -match "(?m)^smoothing=off$" -and $t -match "(?m)^hud=original$" -and $t -match "(?m)^vegetation=off$" -and $t -match "(?m)^fog=classic$" -and $t -notmatch "(?m)^lighting=" -and $t -notmatch "(?m)^texfilter=" -and $t -notmatch "culling" -and (Get-GfxPresetIndex) -eq 0)
& $S.presetCtl.Tag.onPick $null 1
Test-Check 'settings: preset Medium detected' ((Get-GfxPresetIndex) -eq 1 -and $S.presetCtl.Tag.sel -eq 1)
& $S.presetCtl.Tag.onPick $null 2
$t = Opts
Test-Check 'settings: preset High = the defaults: no picture key left' (((@($GfxPresetKeys | Where-Object { $t -match "(?m)^$_=" })).Count -eq 0) -and (Get-GfxPresetIndex) -eq 2 -and $t -match "(?m)^lang=de$")
Pick 'lighting' 1
Test-Check 'settings: Lighting Enhanced makes it Custom and the Enhanced rows lively' ((Get-GfxPresetIndex) -eq 3 -and -not (Ctx 'ao').ctl.Tag.muted)
Pick 'fps' 5; Pick 'renderscale' 3; Pick 'hudpos' 1
$t = Opts
Test-Check 'settings: Display rows: fps=off, renderscale=125, hudpos=corners' ($t -match "(?m)^fps=off$" -and $t -match "(?m)^renderscale=125$" -and $t -match "(?m)^hudpos=corners$")
Pick 'renderscale' 2; Pick 'hudpos' 0; Pick 'fps' 0
$t = Opts
Test-Check 'settings: Display defaults remove their lines' ($t -notmatch 'renderscale|hudpos|fps=')
Pick 'camera' 1; $t = Opts
$ok1 = ($t -match "(?m)^camera=modern$" -and $t -match "(?m)^camfeel=static$")
Pick 'camera' 0; $t = Opts
$ok2 = ($t -match "(?m)^camera=classic$" -and $t -match "(?m)^camfeel=static$")
Pick 'camera' 2; $t = Opts
Test-Check 'settings: Camera Static / Classic / Smooth write camera= and camfeel= as the game does' ($ok1 -and $ok2 -and $t -match "(?m)^camera=modern$" -and $t -notmatch 'camfeel')
Set-RowPct (Ctx 'camspeed') 73; $ok1 = ((Opts) -match "(?m)^camspeed=70$" -and (Ctx 'camspeed').vlbl.Text -eq '70%' -and (Own-Visible (Ctx 'camspeed').lnk))
Set-RowPct (Ctx 'camspeed') $null; $ok2 = ((Opts) -notmatch 'camspeed')
Set-RowPct (Ctx 'hudscale') 100; $ok3 = ((Opts) -notmatch 'hudscale')
Set-RowPct (Ctx 'hudscale') 120
Test-Check 'settings: sliders: camspeed 73 -> 70, Use default, HUD scale 100 = no line, 120 written' ($ok1 -and $ok2 -and $ok3 -and (Opts) -match "(?m)^hudscale=120$")
Pick 'culling' 1; $t = Opts
$ok1 = ($t -match "(?m)^culling=off`nculling_off=kept$")
$S.opt = Read-PcOptions; $ok2 = ((Get-Opt 'culling') -eq 'off' -and (Get-GfxPresetIndex) -eq 3)
& $S.presetCtl.Tag.onPick $null 2; $t = Opts
Test-Check 'settings: Culling Off writes its marker, reads back Off, a preset sets it On' ($ok1 -and $ok2 -and $t -notmatch 'culling')
Pick 'debug' 1; $ok1 = ((Opts) -match "(?m)^debug=on$"); Pick 'debug' 0
Pick 'lang' 1; Pick 'skipcut' 1; Pick 'intro' 0
$t = Opts
Test-Check 'settings: debug on / off (off has no line), language fr, Start skips cutscenes off, Skip intro on' ($ok1 -and $t -notmatch 'debug' -and $t -match "(?m)^lang=fr$" -and $t -match "(?m)^skipcut=off$" -and $t -match "(?m)^intro=skip$")
Shot-Page '11_settings_changed' $PageView.settings
$S.testGameRunning = $true; $before = Opts; Pick 'water' 3; $S.testGameRunning = $false
Test-Check 'settings: nothing is written while the game runs' ((Opts) -eq $before)
Reset-PcOptions
$t = Opts
Test-Check 'settings: Reset keeps only the keys, the unknown line and the version line' ($t -eq "version=1`nkeys=cross:KeyK`nfuture_thing=7`n")
Write-Host ('    after reset: ' + ($t -replace "`n", ' | '))

# ---- Mods
Show-View 'installed'; $S.pageBack = 'installed'; Show-View 'mods'
$ids = @($S.mods.entries | ForEach-Object { $_.id })
Test-Check "mods: six mods listed ($($ids -join ', '))" ($ids.Count -eq 6 -and ($ids -contains 'arsenal-mastery') -and ($ids -contains 'loose-pack') -and ($ids -contains 'broken-mod'))
Test-Check 'mods: active.txt read (HD Pack, Arsenal Mastery, a missing id kept), the comment above HD Pack kept' (($S.mods.active -join ',') -eq 'hd-pack,arsenal-mastery,not-installed-id' -and $S.mods.kept.Count -eq 1 -and $S.mods.kept[0].anchor -eq 'hd-pack')
Test-Check 'mods: the broken manifest and the other game''s mod cannot be switched on' ((Get-ModBlock (Get-ModEntry 'broken-mod')) -and (Get-ModBlock (Get-ModEntry 'other-game')) -match 'another game' -and -not (Get-ModBlock (Get-ModEntry 'keys-demo')) -and -not (Get-ModBlock (Get-ModEntry 'loose-pack')))
Write-Host ('    broken-mod: ' + (Get-ModBlock (Get-ModEntry 'broken-mod')))
Shot-Page '20_mods' $PageView.mods
Set-ModOn 'keys-demo' $true
Test-Check 'mods: switching on puts it at the top' ($S.mods.active[0] -eq 'keys-demo' -and (Active) -match "(?m)^keys-demo`n# my note about the HD pack`nhd-pack`narsenal-mastery`nnot-installed-id$")
Set-ModOn 'broken-mod' $true
Test-Check 'mods: a broken mod is refused' (-not $S.mods.active.Contains('broken-mod') -and $PageView.mods.note.Text -like '*cannot be switched on*')
Set-ModMaster 'textures' $false
$t = Active
Test-Check 'mods: Load texture packs off writes its marker and holds the packs back as # lines' ($t -match "(?m)^# Load texture packs: off$" -and $t -match "(?m)^# hd-pack$" -and $t -match "(?m)^keys-demo$" -and $t -match "(?m)^# my note about the HD pack$")
$S.mods = Read-ModStore
Test-Check 'mods: read back, the held pack is still switched on and the tick off' ($S.mods.off.textures -and $S.mods.active.Contains('hd-pack') -and -not (Test-ModLoaded 'hd-pack'))
Shot-Page '21_mods_textures_off' $PageView.mods
Set-ModMaster 'textures' $true
$t = Active
Test-Check 'mods: on again: plain lines, the same order' ($t -eq "# Enabled mods, one id per line, highest priority first. Written by the RAC-RS Launcher.`nkeys-demo`n# my note about the HD pack`nhd-pack`narsenal-mastery`nnot-installed-id`n")
Move-Mod 'arsenal-mastery' -1
Test-Check 'mods: Arsenal Mastery one up, past the other script mod only' (($S.mods.active -join ',') -eq 'arsenal-mastery,hd-pack,keys-demo,not-installed-id')
Move-Mod 'arsenal-mastery' 1
Test-Check 'mods: and down again' (($S.mods.active -join ',') -eq 'keys-demo,hd-pack,arsenal-mastery,not-installed-id')
# Arsenal Mastery's settings
$S.modOpenSet = 'arsenal-mastery'; Request-PageBuild 'mods'
$am = Get-ModEntry 'arsenal-mastery'
$cur = Get-ModValues $am.sm (Get-StoredModValues 'arsenal-mastery')
Test-Check 'mods: Arsenal Mastery: 13 settings, 4 presets, the stored XP rate 2.0 is used' ($am.sm.settings.Count -eq 13 -and $am.sm.presets.Count -eq 4 -and [double]$cur.xp_rate -eq 2.0 -and $null -eq (Get-LitPreset $am.sm $cur))
Shot-Page '22_mods_arsenal_settings' $PageView.mods
& $S.presetPick @{ id = 'arsenal-mastery'; sm = $am.sm; off = 1 } 3
$j = (SetJson | ConvertFrom-Json)
Test-Check 'mods: preset Survival writes its values, keeps unknown keys and other mods' ($j.format -eq 1 -and $j.mods.'arsenal-mastery'.rebalance -eq 'hard' -and $j.mods.'arsenal-mastery'.ammo_mode -eq 'survival' -and $j.mods.'arsenal-mastery'.unknown_key -eq 5 -and $j.mods.'gone-mod'.a -eq 1)
Test-Check 'mods: a float is written as a float (xp_rate: 1.0)' ((SetJson) -match '"xp_rate": 1\.0[,\r\n]')
$cur = Get-ModValues $am.sm (Get-StoredModValues 'arsenal-mastery')
Test-Check 'mods: the Survival preset is lit' ((Get-LitPreset $am.sm $cur) -eq 2)
Set-ModSetting 'arsenal-mastery' @(@{ key = 'xp_rate'; value = [double]2.5 }, @{ key = 'nanotech'; value = $false })
$j = (SetJson | ConvertFrom-Json)
Test-Check 'mods: a slider and a switch write their values' ([double]$j.mods.'arsenal-mastery'.xp_rate -eq 2.5 -and $j.mods.'arsenal-mastery'.nanotech -eq $false)
$S.modAdv['arsenal-mastery'] = $true; Request-PageBuild 'mods'
Shot-Page '23_mods_arsenal_advanced' $PageView.mods
Set-ModSetting 'arsenal-mastery' @($am.sm.settings | ForEach-Object { @{ key = $_.key; remove = $true } })
$j = (SetJson | ConvertFrom-Json)
Test-Check 'mods: Reset to defaults removes the declared keys only' (@($j.mods.'arsenal-mastery'.PSObject.Properties).Count -eq 1 -and $j.mods.'arsenal-mastery'.unknown_key -eq 5)
# a mod with keybinds, text, numbers and a long choice
$S.modOpenSet = 'keys-demo'; $S.modAdv = @{}; Request-PageBuild 'mods'
Shot-Page '24_mods_keys_demo' $PageView.mods
$S.modListen = @{ id = 'keys-demo'; key = 'jump'; slot = 1 }; Request-PageBuild 'mods'
$menuBtn = $null
$walkB = { param($p) foreach ($c in $p.Controls) { if ($c -is [System.Windows.Forms.Button] -and $c.Text -like 'The first way*') { $script:menuBtn = $c }; if ($c.Controls.Count -gt 0) { & $walkB $c } } }
& $walkB $PageView.mods.content
Test-Check 'mods: a choice too wide for a strip is a button showing the chosen value' ($null -ne $menuBtn)
Test-Check 'mods: the listening keybind button is there' ($null -ne $S.listenBtn -and $S.listenBtn.Text -like 'Press a key*')
Shot-Page '25_mods_keybind_listening' $PageView.mods
Set-ModBindInput $S.listenBtn.Tag (Get-BindNameOfKey ([System.Windows.Forms.Keys]::J)) $false
Test-Check 'mods: a key in input 2 of Jump: "Space,KeyJ"' ((SetJson | ConvertFrom-Json).mods.'keys-demo'.jump -eq 'Space,KeyJ')
$S.modListen = @{ id = 'keys-demo'; key = 'jump'; slot = 0 }; Request-PageBuild 'mods'
Set-ModBindInput $S.listenBtn.Tag 'KeyJ' $false
Test-Check 'mods: the same key in input 1 moves there: "KeyJ"' ((SetJson | ConvertFrom-Json).mods.'keys-demo'.jump -eq 'KeyJ')
Test-Check 'mods: key names (J, comma, numpad 5, F13 none, Escape cancels)' ((Get-BindNameOfKey 'J') -eq 'KeyJ' -and (Get-BindNameOfKey 'Oemcomma') -eq 'Comma' -and (Get-BindNameOfKey 'NumPad5') -eq 'Numpad5' -and $null -eq (Get-BindNameOfKey 'F13') -and (Get-BindFriendly 'ArrowUp') -eq 'Up arrow')
Set-ModSetting 'keys-demo' @(@{ key = 'colour'; value = 'pink' }, @{ key = 'title'; value = 'Hi there' }, @{ key = 'count'; value = [long]42 })
$j = (SetJson | ConvertFrom-Json).mods.'keys-demo'
Test-Check 'mods: a long choice, a text and a number box write their values' ($j.colour -eq 'pink' -and $j.title -eq 'Hi there' -and $j.count -eq 42)
# remove
Remove-Mod 'other-game'
Test-Check 'mods: Remove takes the folder away' (-not (Test-Path -LiteralPath (Join-Path $md 'other-game')) -and -not (Get-ModEntry 'other-game'))
# add a PCSX2 pack from a folder (its name is the serial; one replacements folder; a file that is not a texture rides along)
$pack = Join-Path $src 'SCES-50916'
Put (Join-Path $pack 'replacements\0123456789abcdef-89abcdef.png') 'png'
Put (Join-Path $pack 'replacements\sub\fedcba9876543210-01234567-r64x64-00000001.dds') 'dds'
Put (Join-Path $pack 'dumps\ignored.png') 'x'
$S.modOpenSet = ''
Start-ModInspect $pack
[void](Wait-Until { $S.modJob.phase -ne 'reading' } 30)
Test-Check "mods: PCSX2 folder read: a PCSX2 pack made for SCES-50916, 2 files ($($S.modJob.phase) $($S.modJob.msg))" ($S.modJob.phase -eq 'preview' -and $S.modJob.sync.layout -eq 'pcsx2' -and $S.modJob.sync.serial -eq 'SCES-50916' -and $S.modJob.sync.files.Count -eq 2)
Shot-Page '26_mods_add_preview' $PageView.mods
$S.modJob.nameBox.Text = 'My HD Pack'
Start-ModImport
[void](Wait-Until { -not $S.modJob } 60)
$nd = Join-Path $md 'my-hd-pack'
$mj = $(if (Test-Path -LiteralPath (Join-Path $nd 'mod.json')) { [System.IO.File]::ReadAllText((Join-Path $nd 'mod.json')) | ConvertFrom-Json } else { $null })
Test-Check 'mods: the pack is in mods\my-hd-pack with textures\ and a mod.json, switched on at the top' ($mj -and $mj.kind -eq 'textures' -and $mj.serials[0] -eq 'SCES-50916' -and $mj.name -eq 'My HD Pack' -and (Test-Path -LiteralPath (Join-Path $nd 'textures\sub\fedcba9876543210-01234567-r64x64-00000001.dds')) -and $S.mods.active[0] -eq 'my-hd-pack' -and -not (Test-Path -LiteralPath (Join-Path $md 'my-hd-pack.tmp')))
Write-Host ('    note: ' + $PageView.mods.note.Text)
# add a script mod from a .zip (GitHub style: one folder in it, not named like the mod)
Add-Type -AssemblyName System.IO.Compression.FileSystem
$zs = Join-Path $src 'zipsrc'
Put (Join-Path $zs 'demo-mod-main\omni-mod.json') '{"format":2,"id":"demo-mod","name":"Demo Mod","version":"0.1.0","type":"script","games":{"rac1":["pal"]},"settings":[{"key":"on","name":"Enabled thing","type":"bool","default":true}]}'
Put (Join-Path $zs 'demo-mod-main\main.luau') '-- hello'
$zip = Join-Path $src 'demo-mod-main.zip'
[System.IO.Compression.ZipFile]::CreateFromDirectory($zs, $zip)
Start-ModInspect $zip
[void](Wait-Until { $S.modJob.phase -ne 'reading' } 30)
Test-Check "mods: zip read: a script mod, demo-mod ($($S.modJob.phase) $($S.modJob.msg))" ($S.modJob.phase -eq 'preview' -and $S.modJob.sync.layout -eq 'script' -and $S.modJob.sync.info.id -eq 'demo-mod')
Start-ModImport
[void](Wait-Until { -not $S.modJob } 60)
Test-Check 'mods: the script mod is in mods\demo-mod (its own files only) and on' ((Test-Path -LiteralPath (Join-Path $md 'demo-mod\main.luau')) -and (Test-Path -LiteralPath (Join-Path $md 'demo-mod\omni-mod.json')) -and $S.mods.active.Contains('demo-mod') -and (Get-ModEntry 'demo-mod').kind -eq 'script')
Start-ModInspect $zip
[void](Wait-Until { $S.modJob.phase -ne 'reading' } 30)
Start-ModImport
Test-Check 'mods: the same script mod again is refused (already installed)' ($S.modJob.phase -eq 'error' -and $S.modJob.msg -like '*already installed*')
Shot-Page '27_mods_error' $PageView.mods
$S.modJob = $null
# a mod.json pack whose id is taken: hd-pack-2, its other mod.json keys kept
$z2s = Join-Path $src 'zip2'
Put (Join-Path $z2s 'mod.json') '{"format":1,"id":"hd-pack","name":"HD Pack","kind":"textures","mip_names":"single"}'
Put (Join-Path $z2s 'textures\0123456789abcdef-89abcdef.png') 'x'
$zip2 = Join-Path $src 'hd.zip'
[System.IO.Compression.ZipFile]::CreateFromDirectory($z2s, $zip2)
Start-ModInspect $zip2
[void](Wait-Until { $S.modJob.phase -ne 'reading' } 30)
Start-ModImport
[void](Wait-Until { -not $S.modJob } 60)
$mj = $(if (Test-Path -LiteralPath (Join-Path $md 'hd-pack-2\mod.json')) { [System.IO.File]::ReadAllText((Join-Path $md 'hd-pack-2\mod.json')) | ConvertFrom-Json } else { $null })
Test-Check 'mods: a taken id becomes hd-pack-2, mip_names kept' ($mj -and $mj.id -eq 'hd-pack-2' -and $mj.mip_names -eq 'single' -and (Test-Path -LiteralPath (Join-Path $md 'hd-pack-2\textures\0123456789abcdef-89abcdef.png')))
# what cannot be added
Put (Join-Path $src 'pack.7z') 'x'
Start-ModInspect (Join-Path $src 'pack.7z')
[void](Wait-Until { $S.modJob.phase -ne 'reading' } 30)
Test-Check 'mods: a .7z is refused with a reason' ($S.modJob.phase -eq 'error' -and $S.modJob.msg -like '*.zip files and folders only*')
Put (Join-Path $src 'nothing\readme.txt') 'x'
Start-ModInspect (Join-Path $src 'nothing')
[void](Wait-Until { $S.modJob.phase -ne 'reading' } 30)
Test-Check 'mods: a folder with no mod in it is refused' ($S.modJob.phase -eq 'error' -and $S.modJob.msg -like 'This does not look like a mod*')
$S.modJob = $null
# the PCSX2 list, and a damaged settings.json
$r = Get-Pcsx2Candidates
Test-Check 'mods: Import from PCSX2 looks in Documents\PCSX2\textures' ($r.where -like '*PCSX2\textures\SC*')
Put (Join-Path $md 'settings.json') '{ "format": 1, "mods": { oops'
$S.modOpenSet = 'demo-mod'; Request-PageBuild 'mods'
Test-Check 'mods: a damaged settings.json is noticed' ([bool]$S.modSet.broken)
Shot-Page '28_mods_broken_settings' $PageView.mods
Set-ModSetting 'demo-mod' @(@{ key = 'on'; value = $false })
Test-Check 'mods: the first change keeps the damaged file as settings.json.broken' ((Test-Path -LiteralPath (Join-Path $md 'settings.json.broken')) -and (SetJson | ConvertFrom-Json).mods.'demo-mod'.on -eq $false)
Put (Join-Path $md 'settings.json') '{"format":2,"mods":{}}'
$S.modOpenSet = 'demo-mod'; Request-PageBuild 'mods'
$before = SetJson
Set-ModSetting 'demo-mod' @(@{ key = 'on'; value = $true })
Test-Check 'mods: a settings.json of a newer game is never written' ((SetJson) -eq $before -and $PageView.mods.note.Text -like '*newer*')
Shot-Page '29_mods_end' $PageView.mods
Show-View $S.pageBack
Test-Check 'Back returns to the home view' ($S.view -eq 'installed')
