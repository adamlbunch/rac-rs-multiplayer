# launcher_pages.ps1 - [MP] the Settings and Mods pages of this fork's setup / launcher window (setup.ps1 dot-sources it inside its
# window block and uses that block's helpers, colours, fonts, $S, $UI, $root and $LayoutK). They set what the RAC-RS Launcher's Settings
# and Mods tabs set, which this fork does not offer (that launcher updates from upstream rac-rs, which has no multiplayer).
#
# Settings: player\pc_options.txt, the file the game's own Pause -> Options menus edit (rac-rs\src\retail\pcopts.rs and pcdisp.rs give its
# format). Every click is written at once (temporary file + rename, UTF-8 without BOM, LF); the game reads it at its next start. Lines this
# page does not show (the keyboard and controller buttons, the GPU HUD guard, anything newer) are kept as they are. The graphics presets are
# rac-rs\assets\gfx_presets.txt, the table the game's own Preset row reads.
#
# Mods: the mods\ folder (rac-rs\src\texpack\store.rs and active.rs, rac-rs\src\omni\plugins.rs and modset.rs): mods\active.txt in exactly
# the RAC-RS Launcher's format (the two master ticks, the priority order, lines this page did not write kept in place), mods\settings.json
# ({"format":1,"mods":{"<id>":{"<key>": value}}}, unknown mods and keys kept) for the script mods' settings, read from each mod's own
# omni-mod.json (script and native mods) or mod.json (texture packs). Adding a .zip or a folder (a script mod, a mod with a mod.json, or a
# PCSX2 texture pack) and importing PCSX2's own texture folder run in a background runspace; removing a mod sends it to the Recycle Bin.
# Nothing here is changed while the game runs: it uses these files itself.

# ================================================================ common: sizes, colours, small painted controls
function Px([double]$v) { return [int][math]::Round($v * $LayoutK) }
$fRow = New-Font 'Segoe UI Semibold' 9.5
$fRowBig = New-Font 'Segoe UI Semibold' 10.5
$fGroup = New-Font $HeadFamily 12
$fSeg = New-Font 'Segoe UI' 9
$fChip = New-Font 'Segoe UI Semibold' 7.5
$cCard = [System.Drawing.Color]::FromArgb(17, 25, 52)
$cEdge = [System.Drawing.Color]::FromArgb(44, 60, 102)
$cDim = [System.Drawing.Color]::FromArgb(112, 124, 156)
$cLockTxt = [System.Drawing.Color]::FromArgb(78, 90, 122)
$cRail = [System.Drawing.Color]::FromArgb(52, 62, 94)
$cChipBg = [System.Drawing.Color]::FromArgb(26, 52, 92)
$TfWrap = [System.Windows.Forms.TextFormatFlags]::WordBreak -bor [System.Windows.Forms.TextFormatFlags]::NoPrefix
$TfLine = [System.Windows.Forms.TextFormatFlags]::NoPrefix -bor [System.Windows.Forms.TextFormatFlags]::SingleLine
$TfCenter = $TfLine -bor [System.Windows.Forms.TextFormatFlags]::HorizontalCenter -bor [System.Windows.Forms.TextFormatFlags]::VerticalCenter
$DbProp = [System.Windows.Forms.Control].GetProperty('DoubleBuffered', [Reflection.BindingFlags]'NonPublic,Instance')
function Set-DoubleBuffered($c) { try { $DbProp.SetValue($c, $true, $null) } catch { } }

$PageView = @{}
$S.pagesRoot = $root          # where player\ and mods\ are (the page test points it at a folder of its own)
$S.pageBack = 'installed'; $S.pageBuild = ''; $S.opt = $null; $S.presets = $null; $S.setCtl = @()
$S.mods = $null; $S.modSet = $null; $S.modOpenSet = ''; $S.modOpenInfo = ''; $S.modAdv = @{}; $S.modListen = $null; $S.modJob = $null
$S.listenBtn = $null; $S.listenAt = [datetime]::MinValue; $S.modJobLbl = $null; $S.modJobBar = $null; $S.testGameRunning = $false
function Get-PlayerDir { return (Join-Path $S.pagesRoot 'player') }
function Get-ModsDir { return (Join-Path $S.pagesRoot 'mods') }

function Measure-TextH([string]$text, $font, [int]$w) {
    if (-not $text) { return 0 }
    return [System.Windows.Forms.TextRenderer]::MeasureText($text, $font, (New-Object System.Drawing.Size([math]::Max(1, $w), 100000)), $TfWrap).Height
}
function Measure-TextW([string]$text, $font) {
    if (-not $text) { return 0 }
    return [System.Windows.Forms.TextRenderer]::MeasureText($text, $font, (New-Object System.Drawing.Size(100000, 1000)), $TfLine).Width
}
function Add-Text($parent, [string]$text, [int]$x, [int]$y, [int]$w, $font, $color) {
    # a label as tall as its text needs at width $w, on the parent's own colour (no transparency: the pages scroll)
    $l = New-Label $text $x $y $w ((Measure-TextH $text $font $w) + 2) $font $color
    $l.BackColor = $parent.BackColor
    $parent.Controls.Add($l)
    return $l
}
function Add-PageButton($parent, [string]$text, [int]$x, [int]$y, $tag, [scriptblock]$onClick, [int]$minW = 0) {
    # a theme button (pixels) as wide as its text needs
    $w = [math]::Max($minW, (Measure-TextW $text $fN) + (Px 26))
    $b = New-Button $text $x $y $w (Px 28) $false
    $b.Tag = $tag
    if ($onClick) { $b.Add_Click($onClick) }
    $parent.Controls.Add($b)
    return $b
}
function Add-Line($parent, [int]$x, [int]$y, [int]$w) {
    $l = New-Object System.Windows.Forms.Panel
    $l.Location = New-Object System.Drawing.Point($x, $y); $l.Size = New-Object System.Drawing.Size($w, [math]::Max(1, (Px 1)))
    $l.BackColor = $cEdge
    $parent.Controls.Add($l)
}

function Paint-Card($c, $e) {
    # a rounded card: the corners outside the round edge in the colour behind the card, then the edge (orange for a mod that is loaded)
    $g = $e.Graphics; $w = $c.ClientSize.Width; $h = $c.ClientSize.Height
    if ($w -lt 6 -or $h -lt 6) { return }
    $g.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::AntiAlias
    $path = New-RoundPath 0.5 0.5 ([single]($w - 1.5)) ([single]($h - 1.5)) ([single](Px 10))
    $reg = New-Object System.Drawing.Region((New-Object System.Drawing.Rectangle(0, 0, $w, $h)))
    $reg.Exclude($path)
    $out = $(if ($c.Parent) { $c.Parent.BackColor } else { $cBg })
    $br = New-Object System.Drawing.SolidBrush($out); $g.FillRegion($br, $reg); $br.Dispose(); $reg.Dispose()
    $edge = $(if ($c.Tag -is [System.Drawing.Color]) { $c.Tag } else { $cEdge })
    $pen = New-Object System.Drawing.Pen($edge, [single][math]::Max(1.0, $LayoutK)); $g.DrawPath($pen, $path); $pen.Dispose(); $path.Dispose()
}
function New-Card($parent, [int]$y, $edge = $null) {
    $c = New-Object System.Windows.Forms.Panel
    $c.Location = New-Object System.Drawing.Point(0, $y); $c.Size = New-Object System.Drawing.Size($parent.Width, (Px 20))
    $c.BackColor = $cCard; $c.Tag = $edge
    Set-DoubleBuffered $c
    $c.Add_Paint({ param($sx, $e) try { Paint-Card $sx $e } catch { } })
    $parent.Controls.Add($c)
    return $c
}
function Add-GroupTitle($parent, [int]$y, [string]$text) {
    $l = Add-Text $parent $text (Px 4) $y ($parent.Width - (Px 8)) $fGroup $cAccent
    return ($l.Bottom + (Px 2))
}

# --- segmented control: one rounded strip, the chosen part orange. Tag: labels, sel, hot, widths, locked parts, onPick { param($ctx, $i) }.
function New-Segmented([string[]]$labels, [int]$sel, [scriptblock]$onPick, $ctx) {
    $pad = Px 11; $minW = Px 38
    $ws = New-Object 'System.Collections.Generic.List[int]'
    foreach ($t in $labels) { $ws.Add([int][math]::Max($minW, (Measure-TextW $t $fSeg) + 2 * $pad)) }
    $tw = 0; foreach ($x in $ws) { $tw += $x }
    $p = New-Object System.Windows.Forms.Panel
    Set-DoubleBuffered $p
    $p.Size = New-Object System.Drawing.Size(($tw + 4), (Px 28))
    $p.Cursor = [System.Windows.Forms.Cursors]::Hand
    $p.Tag = @{ labels = $labels; sel = $sel; hot = -1; ws = $ws; locked = @(); onPick = $onPick; ctx = $ctx; muted = $false }
    $p.Add_Paint({ param($sx, $e) try { Paint-Segmented $sx $e } catch { } })
    $p.Add_MouseMove({ param($sx, $e) $i = Get-SegAt $sx $e.X; if ($i -ne $sx.Tag.hot) { $sx.Tag.hot = $i; $sx.Invalidate() } })
    $p.Add_MouseLeave({ param($sx, $e) if ($sx.Tag.hot -ne -1) { $sx.Tag.hot = -1; $sx.Invalidate() } })
    $p.Add_MouseClick({
        param($sx, $e)
        if ($e.Button -ne [System.Windows.Forms.MouseButtons]::Left) { return }
        $i = Get-SegAt $sx $e.X
        if ($i -lt 0 -or $i -eq $sx.Tag.sel -or (Test-SegLocked $sx $i)) { return }
        try { & $sx.Tag.onPick $sx.Tag.ctx $i } catch { Show-PageError $_ }
    })
    return $p
}
function Get-SegAt($p, [int]$x) {
    $acc = 2
    for ($i = 0; $i -lt $p.Tag.ws.Count; $i++) { $acc += $p.Tag.ws[$i]; if ($x -lt $acc) { return $i } }
    return -1
}
function Test-SegLocked($p, [int]$i) { $l = @($p.Tag.locked); return ($i -lt $l.Count -and [bool]$l[$i]) }
function Paint-Segmented($p, $e) {
    $g = $e.Graphics; $t = $p.Tag; $w = $p.ClientSize.Width; $h = $p.ClientSize.Height
    $g.Clear($p.Parent.BackColor)
    $g.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::AntiAlias
    $r = [single](Px 7)
    $path = New-RoundPath 0.5 0.5 ([single]($w - 1.5)) ([single]($h - 1.5)) $r
    $br = New-Object System.Drawing.SolidBrush($cField); $g.FillPath($br, $path); $br.Dispose()
    $pen = New-Object System.Drawing.Pen($cEdge, [single][math]::Max(1.0, $LayoutK)); $g.DrawPath($pen, $path); $pen.Dispose(); $path.Dispose()
    $x = 2
    for ($i = 0; $i -lt $t.ws.Count; $i++) {
        $sw = $t.ws[$i]
        $lock = Test-SegLocked $p $i
        if ($i -eq $t.sel -or ($i -eq $t.hot -and -not $lock)) {
            $sp = New-RoundPath ([single]($x + 0.5)) ([single]2.5) ([single]($sw - 1)) ([single]($h - 5)) ([single][math]::Max(2.0, $r - 2))
            if ($i -eq $t.sel) {
                $c1 = $(if ($t.muted) { Get-Tint $cAccent -0.4 } else { $cAccent })
                $lb = New-Object System.Drawing.Drawing2D.LinearGradientBrush((New-Object System.Drawing.Rectangle(0, 0, $w, $h)), (Get-Tint $c1 0.18), (Get-Tint $c1 -0.12), [single]90)
                $g.FillPath($lb, $sp); $lb.Dispose()
            } else {
                $hb = New-Object System.Drawing.SolidBrush($cBtn2); $g.FillPath($hb, $sp); $hb.Dispose()
            }
            $sp.Dispose()
        }
        $fc = $(if ($i -eq $t.sel) { [System.Drawing.Color]::White } elseif ($lock) { $cLockTxt } elseif ($t.muted) { $cDim } else { $cText })
        [System.Windows.Forms.TextRenderer]::DrawText($g, [string]$t.labels[$i], $fSeg, (New-Object System.Drawing.Rectangle($x, 0, $sw, $h)), $fc, $TfCenter)
        $x += $sw
    }
}

# --- switch: a pill with a knob. Tag: on, hot, onFlip { param($ctx, $on) }.
function New-Switch([bool]$on, [scriptblock]$onFlip, $ctx, [bool]$enabled = $true) {
    $p = New-Object System.Windows.Forms.Panel
    Set-DoubleBuffered $p
    $p.Size = New-Object System.Drawing.Size((Px 42), (Px 24))
    if ($enabled) { $p.Cursor = [System.Windows.Forms.Cursors]::Hand }
    $p.Tag = @{ on = $on; hot = $false; onFlip = $onFlip; ctx = $ctx; enabled = $enabled; muted = $false }
    $p.Add_Paint({ param($sx, $e) try { Paint-Switch $sx $e } catch { } })
    $p.Add_MouseEnter({ param($sx, $e) $sx.Tag.hot = $true; $sx.Invalidate() })
    $p.Add_MouseLeave({ param($sx, $e) $sx.Tag.hot = $false; $sx.Invalidate() })
    $p.Add_MouseClick({
        param($sx, $e)
        if ($e.Button -ne [System.Windows.Forms.MouseButtons]::Left -or -not $sx.Tag.enabled) { return }
        try { & $sx.Tag.onFlip $sx.Tag.ctx (-not $sx.Tag.on) } catch { Show-PageError $_ }
    })
    return $p
}
function Paint-Switch($p, $e) {
    $g = $e.Graphics; $t = $p.Tag; $w = $p.ClientSize.Width; $h = $p.ClientSize.Height
    $g.Clear($p.Parent.BackColor)
    $g.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::AntiAlias
    $path = New-RoundPath 1 1 ([single]($w - 2)) ([single]($h - 2)) ([single](($h - 2) / 2.0))
    $base = $(if (-not $t.enabled) { $cOff } elseif ($t.on -and $t.muted) { Get-Tint $cAccent -0.45 } elseif ($t.on) { $cAccent } else { $cRail })
    if ($t.hot -and $t.enabled) { $base = Get-Tint $base 0.12 }
    $br = New-Object System.Drawing.SolidBrush($base); $g.FillPath($br, $path); $br.Dispose()
    $pen = New-Object System.Drawing.Pen($(if ($t.on) { Get-Tint $base -0.35 } else { $cEdge }), [single][math]::Max(1.0, $LayoutK)); $g.DrawPath($pen, $path); $pen.Dispose(); $path.Dispose()
    $d = [single]($h - 8)
    $kx = $(if ($t.on) { [single]($w - 4 - $d) } else { [single]4 })
    $kb = New-Object System.Drawing.SolidBrush($(if ($t.enabled) { [System.Drawing.Color]::White } else { $cDim })); $g.FillEllipse($kb, $kx, [single]4, $d, $d); $kb.Dispose()
}

# --- slider: a rail, its filled part orange, a round knob. Tag: min, max, step, value; onMove { param($ctx, $v) } while dragging, onDone when let go.
function New-Slider([double]$min, [double]$max, [double]$step, [double]$value, [int]$w, [scriptblock]$onMove, [scriptblock]$onDone, $ctx) {
    $p = New-Object System.Windows.Forms.Panel
    Set-DoubleBuffered $p
    $p.Size = New-Object System.Drawing.Size($w, (Px 24))
    $p.Cursor = [System.Windows.Forms.Cursors]::Hand
    $p.Tag = @{ min = $min; max = $max; step = $step; value = [math]::Min($max, [math]::Max($min, $value)); drag = $false; start = $value; onMove = $onMove; onDone = $onDone; ctx = $ctx }
    $p.Add_Paint({ param($sx, $e) try { Paint-Slider $sx $e } catch { } })
    $p.Add_MouseDown({ param($sx, $e) if ($e.Button -eq [System.Windows.Forms.MouseButtons]::Left) { $sx.Tag.drag = $true; $sx.Tag.start = $sx.Tag.value; $sx.Capture = $true; Set-SliderAt $sx $e.X } })
    $p.Add_MouseMove({ param($sx, $e) if ($sx.Tag.drag) { Set-SliderAt $sx $e.X } })
    $p.Add_MouseUp({
        param($sx, $e)
        if (-not $sx.Tag.drag) { return }
        $sx.Tag.drag = $false; $sx.Capture = $false
        if ([math]::Abs($sx.Tag.value - $sx.Tag.start) -gt 1e-9) { try { & $sx.Tag.onDone $sx.Tag.ctx $sx.Tag.value } catch { Show-PageError $_ } }
    })
    return $p
}
function Get-SliderValueAt($p, [int]$x) {
    $t = $p.Tag; $m = Px 9
    $f = ($x - $m) / [double][math]::Max(1, $p.ClientSize.Width - 2 * $m)
    $f = [math]::Min(1.0, [math]::Max(0.0, $f))
    $v = $t.min + $f * ($t.max - $t.min)
    if ($t.step -gt 0) { $v = $t.min + [math]::Round(($v - $t.min) / $t.step) * $t.step }
    $v = [math]::Round($v * 1e6) / 1e6
    return [math]::Min($t.max, [math]::Max($t.min, $v))
}
function Set-SliderAt($p, [int]$x) {
    $v = Get-SliderValueAt $p $x
    if ([math]::Abs($v - $p.Tag.value) -lt 1e-9) { return }
    $p.Tag.value = $v; $p.Invalidate()
    if ($p.Tag.onMove) { try { & $p.Tag.onMove $p.Tag.ctx $v } catch { } }
}
function Paint-Slider($p, $e) {
    $g = $e.Graphics; $t = $p.Tag; $w = $p.ClientSize.Width; $h = $p.ClientSize.Height
    $g.Clear($p.Parent.BackColor)
    $g.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::AntiAlias
    $m = Px 9; $rh = [single][math]::Max(3, (Px 4)); $cy = [single]($h / 2.0)
    $f = $(if ($t.max -gt $t.min) { ($t.value - $t.min) / ($t.max - $t.min) } else { 0 })
    $kx = [single]($m + $f * ($w - 2 * $m))
    $rail = New-RoundPath ([single]$m) ([single]($cy - $rh / 2)) ([single]($w - 2 * $m)) $rh ([single]($rh / 2 - 0.01))
    $br = New-Object System.Drawing.SolidBrush($cRail); $g.FillPath($br, $rail); $br.Dispose(); $rail.Dispose()
    if ($kx -gt $m + 1) {
        $fill = New-RoundPath ([single]$m) ([single]($cy - $rh / 2)) ([single]($kx - $m)) $rh ([single]($rh / 2 - 0.01))
        $br = New-Object System.Drawing.SolidBrush($cAccent); $g.FillPath($br, $fill); $br.Dispose(); $fill.Dispose()
    }
    $kr = [single](Px 8)
    $kb = New-Object System.Drawing.SolidBrush($cText); $g.FillEllipse($kb, [single]($kx - $kr), [single]($cy - $kr), [single](2 * $kr), [single](2 * $kr)); $kb.Dispose()
    $pen = New-Object System.Drawing.Pen($cAccent, [single][math]::Max(2.0, 2 * $LayoutK)); $g.DrawEllipse($pen, [single]($kx - $kr + 1), [single]($cy - $kr + 1), [single](2 * $kr - 2), [single](2 * $kr - 2)); $pen.Dispose()
}

# ================================================================ the page frame (title, subtitle, a scrolling area, Back and a note line)
function New-PageView([string]$title, [string]$sub) {
    $p = New-Panel
    $p.AutoScroll = $false
    $p.Controls.Add((New-Label $title 28 22 524 36 $fTitle $cText))
    $sl = New-Label $sub 28 62 524 38 $fSmall $cGrey
    $p.Controls.Add($sl)
    $sc = New-Object System.Windows.Forms.Panel
    $sc.Location = New-Object System.Drawing.Point(18, 102); $sc.Size = New-Object System.Drawing.Size(544, 428)
    $sc.BackColor = $cBg; $sc.AutoScroll = $true
    $sc.Add_HandleCreated({ param($sx, $e) try { if ($Native) { [void][RacMp.Native]::SetWindowTheme($sx.Handle, 'DarkMode_Explorer', $null) } } catch { } })
    $p.Controls.Add($sc)
    $back = New-Link 'Back' 28 542 80 22
    $p.Controls.Add($back)
    $note = New-Label '' 112 536 440 34 $fSmall $cGrey
    $note.TextAlign = 'MiddleRight'
    $p.Controls.Add($note)
    return @{ panel = $p; scroll = $sc; sub = $sl; back = $back; note = $note; content = $null; subText = $sub }
}
function New-PageContent($pg) {
    # the inner panel a page is built into (rebuilt as a whole; the scroll position is kept), as wide as the area minus a scroll bar
    $c = New-Object System.Windows.Forms.Panel
    $c.Location = New-Object System.Drawing.Point(0, 0)
    $c.Size = New-Object System.Drawing.Size(($pg.scroll.Width - [System.Windows.Forms.SystemInformation]::VerticalScrollBarWidth - 2), 10)
    $c.BackColor = $cBg
    return $c
}
function Set-PageContent($pg, $c) {
    $sc = $pg.scroll
    $pos = 0
    try { $pos = [int]$sc.VerticalScroll.Value } catch { }
    $sc.SuspendLayout()
    try {
        $sc.AutoScrollPosition = New-Object System.Drawing.Point(0, 0)
        $old = $pg.content
        $sc.Controls.Add($c)
        $pg.content = $c
        if ($old) { $sc.Controls.Remove($old); $old.Dispose() }
    } finally { $sc.ResumeLayout() }
    $sc.AutoScrollPosition = New-Object System.Drawing.Point(0, $pos)
    # the hidden test: a window that is never shown makes no handles for controls added later, and DrawToBitmap draws only those with one
    if ($S.test) { try { [void]$c.GetType().GetMethod('CreateControl', [Reflection.BindingFlags]'NonPublic,Instance', $null, [Type[]]@([bool]), $null).Invoke($c, @($true)) } catch { } }
}
function Set-PageNote($pg, [string]$text, $color) {
    $pg.note.Text = $text
    $pg.note.ForeColor = $(if ($color) { $color } else { $cGrey })
    Fit-LabelFont $pg.note
}
function Show-PageError($err) {
    $msg = $(if ($err -is [System.Management.Automation.ErrorRecord]) { $err.Exception.Message } else { [string]$err })
    $pg = $(if ($S.view -eq 'mods') { $PageView.mods } else { $PageView.settings })
    if ($pg) { Set-PageNote $pg ('Something went wrong: ' + $msg) $cRed }
}
function Request-PageBuild([string]$which) {
    # a page rebuilt after the click that asked for it has finished (a control must not be thrown away inside its own event); the hidden
    # test has no message loop, so it builds at once
    if ($S.test) { if ($which -eq 'mods') { Build-ModsPage } else { Build-SettingsPage }; return }
    $S.pageBuild = $which
    $UI.pageTimer.Stop(); $UI.pageTimer.Start()
}
$UI.pageTimer = New-Object System.Windows.Forms.Timer
$UI.pageTimer.Interval = 15
$UI.pageTimer.Add_Tick({
    $UI.pageTimer.Stop()
    $w = $S.pageBuild; $S.pageBuild = ''
    try { if ($w -eq 'mods') { Build-ModsPage } elseif ($w -eq 'settings') { Build-SettingsPage } } catch { Show-PageError $_ }
})
function Test-GameRunning {
    if ($S.test) { return [bool]$S.testGameRunning }
    try { return ((Get-GameProcs).Count -gt 0) } catch { return $false }
}
function Confirm-GameClosed {
    # the game reads these files when it starts and writes them from its own menus while it runs: nothing is changed under it
    if (-not (Test-GameRunning)) { return $true }
    if (-not $S.test) { [void][System.Windows.Forms.MessageBox]::Show($form, 'The game is running. Close it first, then change this here: the game uses these files while it runs and would write over the change.', $AppTitle, 'OK', 'Information') }
    return $false
}
function Write-FileAtomic([string]$path, [string]$text) {
    # temporary file + rename (a crash never leaves half a file), UTF-8 without BOM
    $dir = Split-Path -Parent $path
    if (-not (Test-Path -LiteralPath $dir -PathType Container)) { [void][System.IO.Directory]::CreateDirectory($dir) }
    $tmp = $path + '.tmp'
    [System.IO.File]::WriteAllText($tmp, $text, (New-Object System.Text.UTF8Encoding($false)))
    if (Test-Path -LiteralPath $path -PathType Leaf) {
        try { [System.IO.File]::Replace($tmp, $path, $null) }
        catch { [System.IO.File]::Copy($tmp, $path, $true); [System.IO.File]::Delete($tmp) }
    } else { [System.IO.File]::Move($tmp, $path) }
}
function Open-Path([string]$p) { try { Start-Process -FilePath 'explorer.exe' -ArgumentList ('"' + $p + '"') } catch { } }

# ================================================================ pc_options.txt
# Keys in the order the game writes them (pcopts.rs `to_text`); every other line (keys, pad, hudpos, hudscale, fps, renderscale, gscap_gpu
# and its notes, anything newer) follows as it was. culling_off=kept marks a Culling Off chosen since v0.4.1: without it `culling=off` is a
# v0.4.0 leftover and reads On (the game resets it the same way), and it is written exactly while Culling is Off.
$PcKnown = @('lang', 'camera', 'camfeel', 'strafe', 'buyall', 'lockon', 'camspeed', 'mousespeed', 'lighting', 'ao', 'shadows', 'display', 'water', 'particles', 'sunrays', 'smoothing', 'vegetation', 'culling', 'gi', 'waves', 'draw', 'fog', 'clouds', 'texfilter', 'hud', 'renderer', 'intro', 'skipcut', 'debug')
function Get-Flag([string]$v, [string]$yes, [string]$no) {
    if ($v -eq $yes -or $v -in @('true', '1', 'yes')) { return $yes }
    if ($v -eq $no -or $v -in @('false', '0', 'no')) { return $no }
    return $null
}
function Get-Level([string]$v, [string[]]$names) {
    if ($v -in @('off', 'classic', 'false', '0', 'no')) { return $names[0] }
    if ($v -eq 'low') { return $names[1] }
    if ($v -eq 'medium') { return $names[2] }
    if ($v -in @('high', 'on', 'enhanced', 'true', '1', 'yes')) { return $names[3] }
    return $null
}
function Get-SnapPct([int]$p) { return [int][math]::Min(200, [math]::Max(10, [math]::Floor(([math]::Min($p, 255) + 5) / 10) * 10)) }
function ConvertTo-OptCanon([string]$key, [string]$raw) {
    # the value as the game writes it, or $null when the game cannot read it (the key is then dropped, as the game does)
    $v = $raw.Trim().ToLowerInvariant()
    if ($key -eq 'lang') { if ($v -in @('en', 'fr', 'de', 'es', 'it')) { return $v }; return $null }
    if ($key -eq 'camera') { return (Get-Flag $v 'modern' 'classic') }
    if ($key -eq 'camfeel') { return (Get-Flag $v 'smooth' 'static') }
    if ($key -in @('strafe', 'buyall', 'smoothing', 'culling', 'waves', 'debug', 'skipcut')) { return (Get-Flag $v 'on' 'off') }
    if ($key -eq 'lockon') { if ($v -in @('auto', 'on', 'off')) { return $v }; return $null }
    if ($key -in @('camspeed', 'mousespeed')) { $n = 0; if ([int]::TryParse($v.TrimEnd('%').Trim(), [ref]$n) -and $n -ge 0) { return [string](Get-SnapPct $n) }; return $null }
    if ($key -eq 'lighting') { if ($v -in @('classic', 'original')) { return 'classic' }; if ($v -in @('enhanced', 'modern', 'experimental')) { return 'enhanced' }; return $null }
    if ($key -in @('ao', 'shadows', 'sunrays', 'gi')) { return (Get-Level $v @('off', 'low', 'medium', 'high')) }
    if ($key -in @('water', 'particles', 'fog', 'clouds')) { return (Get-Level $v @('classic', 'low', 'medium', 'high')) }
    if ($key -eq 'display') { return (Get-Flag $v 'fullscreen' 'windowed') }
    if ($key -eq 'renderer') { if ($v -in @('vulkan', 'dx12')) { return $v }; return $null }
    if ($key -eq 'vegetation') { if ($v -in @('off', 'moderate', 'lush')) { return $v }; return $null }
    if ($key -eq 'draw') { if ($v -in @('low', 'medium', 'high')) { return $v }; return $null }
    if ($key -eq 'hud') { return (Get-Flag $v 'sharp' 'original') }
    if ($key -eq 'intro') { return (Get-Flag $v 'skip' 'play') }
    if ($key -eq 'texfilter') { return (Get-Flag $v 'smooth' 'classic') }
    return $null
}
function Read-PcOptions {
    # every line as key -> value (keys lower case, the last line of a key wins), version and comments left out
    $o = [ordered]@{}
    $path = Join-Path (Get-PlayerDir) 'pc_options.txt'
    $text = ''
    if (Test-Path -LiteralPath $path -PathType Leaf) { try { $text = [System.IO.File]::ReadAllText($path, [System.Text.Encoding]::UTF8) } catch { $text = '' } }
    foreach ($line in ($text -split "`r?`n")) {
        $l = $line.TrimStart([char]0xFEFF).Trim()
        $i = $l.IndexOf('=')
        if ($i -lt 0) { continue }
        $k = $l.Substring(0, $i).Trim().ToLowerInvariant(); $v = $l.Substring($i + 1).Trim()
        if (-not $k -or $k.StartsWith('#') -or $k -eq 'version') { continue }
        if ($o.Contains($k)) { $o.Remove($k) }
        $o[$k] = $v
    }
    $kept = ($o.Contains('culling_off') -and ([string]$o['culling_off']).Trim() -ieq 'kept')
    if ($o.Contains('culling_off')) { $o.Remove('culling_off') }
    if ($o.Contains('culling') -and (ConvertTo-OptCanon 'culling' ([string]$o['culling'])) -eq 'off' -and -not $kept) { $o['culling'] = 'on' }
    return $o
}
function Format-PcOptions($o) {
    $sb = New-Object System.Text.StringBuilder
    [void]$sb.Append("version=1`n")
    foreach ($k in $PcKnown) {
        if (-not $o.Contains($k)) { continue }
        $c = ConvertTo-OptCanon $k ([string]$o[$k])
        if ($null -eq $c) { continue }
        if (($k -eq 'camfeel' -and $c -ne 'static') -or ($k -eq 'debug' -and $c -ne 'on')) { continue }      # the game writes only these values (the others are the default)
        [void]$sb.Append("$k=$c`n")
        if ($k -eq 'culling' -and $c -eq 'off') { [void]$sb.Append("culling_off=kept`n") }
    }
    foreach ($k in @($o.get_Keys())) {
        if ($PcKnown -contains $k) { continue }
        [void]$sb.Append("$k=$([string]$o[$k] -replace '[\r\n]', ' ')`n")
    }
    return $sb.ToString()
}
function Get-Opt([string]$k) {
    if (-not $S.opt.Contains($k)) { return $null }
    return (ConvertTo-OptCanon $k ([string]$S.opt[$k]))
}
function Set-OptRaw([string]$k, $v) {
    if ($null -eq $v -or [string]$v -eq '') { if ($S.opt.Contains($k)) { $S.opt.Remove($k) } }
    else { $S.opt[$k] = [string]$v }
}
function Save-PcOptions {
    try { Write-FileAtomic (Join-Path (Get-PlayerDir) 'pc_options.txt') (Format-PcOptions $S.opt); return $true }
    catch { Set-PageNote $PageView.settings ('Your settings could not be saved: ' + $_.Exception.Message) $cRed; return $false }
}
function Get-NearestText([string]$raw, [int[]]$list) {
    $n = 0
    if (-not [int]::TryParse(($raw.Trim().TrimEnd('%').Trim()), [ref]$n) -or $n -le 0) { return $null }
    $best = $list[0]
    foreach ($l in $list) { if ([math]::Abs($l - $n) -lt [math]::Abs($best - $n)) { $best = $l } }
    return [string]$best
}
function Get-ExtraValue([string]$k) {
    # the Display rows (pcdisp.rs: kept as their own lines; a default value has no line): '' = the default
    if (-not $S.opt.Contains($k)) { return '' }
    $raw = ([string]$S.opt[$k]).Trim().ToLowerInvariant()
    if ($k -eq 'hudpos') { if ($raw -eq 'corners') { return 'corners' }; return '' }
    if ($k -eq 'fps') { if ($raw -eq 'off') { return 'off' }; $v = Get-NearestText $raw @(30, 60, 120, 144); if ($v) { return $v }; return '' }
    if ($k -eq 'renderscale') { $v = Get-NearestText $raw @(50, 75, 100, 125, 150, 200); if ($v -and $v -ne '100') { return $v }; return '' }
    if ($k -eq 'hudscale') {
        $n = 0
        if ([int]::TryParse($raw.TrimEnd('%').Trim(), [ref]$n)) {
            $n = [math]::Min(150, [math]::Max(75, $n)); $n = [int]([math]::Floor(($n + 2) / 5) * 5); $n = [math]::Min(150, [math]::Max(75, $n))
            if ($n -ne 100) { return [string]$n }
        }
        return ''
    }
    return ''
}

# ================================================================ the Settings page: its rows (the game's Pause -> Options -> Graphics,
# Camera Options and Controls -> Debug rows, the title's Language)
$OnOff = @(@('on', 'On'), @('off', 'Off'))
$Levels = @(@('off', 'Off'), @('low', 'Low'), @('medium', 'Medium'), @('high', 'High'))
$LevelsC = @(@('classic', 'Classic'), @('low', 'Low'), @('medium', 'Medium'), @('high', 'High'))
$SetRows = @(
    @{ g = 'gfx'; key = 'renderer'; kind = 'choice'; def = 'vulkan'; label = 'Graphics system'; ch = @(@('vulkan', 'Vulkan'), @('dx12', 'DirectX 12')); help = 'What the game draws with. Vulkan is best for most PCs. If the game does not start or the picture is broken, try DirectX 12.' },
    @{ g = 'gfx'; key = 'display'; kind = 'choice'; def = 'windowed'; label = 'Window'; ch = @(@('windowed', 'Windowed'), @('fullscreen', 'Fullscreen')); help = 'Play in a window or fill the screen (without borders).' },
    @{ g = 'gfx'; key = 'lighting'; kind = 'choice'; def = 'classic'; label = 'Lighting'; ch = @(@('classic', 'Classic'), @('enhanced', 'Enhanced')); help = 'Classic is the picture of the PlayStation 2 game. Enhanced adds modern lighting; the effects under Enhanced picture need it.' },
    @{ g = 'gfx'; key = 'draw'; kind = 'choice'; def = 'medium'; label = 'Draw distance'; ch = @(@('low', 'Low'), @('medium', 'Medium'), @('high', 'High')); help = 'How far away you can see. Low is the original distance.' },
    @{ g = 'gfx'; key = 'smoothing'; kind = 'choice'; def = 'on'; label = 'Frame smoothing'; ch = $OnOff; help = 'Adds in-between pictures so movement looks smooth on a fast screen.' },
    @{ g = 'gfx'; key = 'hud'; kind = 'choice'; def = 'sharp'; label = 'Menus and HUD'; ch = @(@('sharp', 'Sharp'), @('original', 'Original')); help = "Sharp draws the menus and the on-screen display again at your screen's resolution. Original keeps the old low-resolution look." },
    @{ g = 'disp'; key = 'hudpos'; kind = 'choice'; extra = $true; def = ''; label = 'HUD position'; ch = @(@('', '4:3'), @('corners', 'Corners')); help = 'Where the bolt and nanotech boxes sit on a wide screen during play. 4:3 keeps them where the original game has them; Corners puts them in the corners of the window.' },
    @{ g = 'disp'; key = 'hudscale'; kind = 'pct'; extra = $true; min = 75; max = 150; step = 5; dflt = 100; deftext = 'Default (100%)'; label = 'HUD scale'; help = 'How big the bolt and nanotech boxes are drawn, in percent.' },
    @{ g = 'disp'; key = 'fps'; kind = 'choice'; extra = $true; def = ''; label = 'FPS limit'; ch = @(@('', 'Display'), @('30', '30'), @('60', '60'), @('120', '120'), @('144', '144'), @('off', 'Off')); help = "The most frames a second the game shows. Display follows your screen's refresh rate; Off sets no limit." },
    @{ g = 'disp'; key = 'renderscale'; kind = 'choice'; extra = $true; def = ''; label = 'Render scale'; ch = @(@('50', '50%'), @('75', '75%'), @('', '100%'), @('125', '125%'), @('150', '150%'), @('200', '200%')); help = 'The size the picture is drawn at, in percent of the window. Below 100% is faster; above 100% is sharper and needs a stronger graphics card.' },
    @{ g = 'enh'; key = 'texfilter'; kind = 'choice'; def = 'smooth'; label = 'Texture filtering'; ch = @(@('classic', 'Classic'), @('smooth', 'Smooth')); help = 'Smooth blends texture detail as you move. Classic keeps the original detail steps.' },
    @{ g = 'enh'; key = 'ao'; kind = 'choice'; def = 'high'; label = 'Ambient occlusion'; ch = $Levels; help = 'Soft shading in corners and creases.' },
    @{ g = 'enh'; key = 'shadows'; kind = 'choice'; def = 'high'; label = 'Dynamic shadows'; ch = $Levels; help = 'Shadows that move with the characters and objects.' },
    @{ g = 'enh'; key = 'water'; kind = 'choice'; def = 'high'; label = 'Water'; ch = $LevelsC; help = 'Reflections and movement of water. Classic is the original look.' },
    @{ g = 'enh'; key = 'waves'; kind = 'choice'; def = 'on'; label = 'Waves'; ch = $OnOff; help = 'Real moving waves on the sea (with Water on Medium or High).' },
    @{ g = 'enh'; key = 'fog'; kind = 'choice'; def = 'high'; label = 'Volumetric fog'; ch = $LevelsC; help = "Volumetric fog that drifts and catches the sun, in each planet's own fog colour. Classic is the original look." },
    @{ g = 'enh'; key = 'clouds'; kind = 'choice'; def = 'high'; label = 'Volumetric clouds'; ch = $LevelsC; help = 'Real 3D clouds that drift and change shape, lit by the sun. Classic is the original sky.' },
    @{ g = 'enh'; key = 'particles'; kind = 'choice'; def = 'high'; label = 'Particles'; ch = $LevelsC; help = 'Smoke, sparks, fire and other effects. Classic is the original look.' },
    @{ g = 'enh'; key = 'sunrays'; kind = 'choice'; def = 'high'; label = 'Sun rays'; ch = $Levels; help = 'Beams of light through the air.' },
    @{ g = 'enh'; key = 'vegetation'; kind = 'choice'; def = 'moderate'; label = 'Vegetation'; ch = @(@('off', 'Off'), @('moderate', 'Moderate'), @('lush', 'Lush')); help = 'How many plants and how lush the greenery is.' },
    @{ g = 'enh'; key = 'gi'; kind = 'choice'; def = 'off'; label = 'Global illumination'; ch = $Levels; help = 'Light that bounces off surfaces. Looks great but needs a strong graphics card.' },
    @{ g = 'play'; key = 'camera'; kind = 'camera'; def = 'smooth'; label = 'Camera'; ch = @(@('classic', 'Classic'), @('static', 'Static'), @('smooth', 'Smooth')); help = 'Classic is the original camera. Static is a plain modern camera. Smooth is the modern camera that follows Ratchet smoothly.' },
    @{ g = 'play'; key = 'camspeed'; kind = 'pct'; min = 10; max = 200; step = 10; dflt = $null; deftext = "Game's own setting"; label = 'Camera speed'; help = "How fast the right stick turns the camera, in percent. Unset, the game's own Slow / Medium / Fast row decides." },
    @{ g = 'play'; key = 'mousespeed'; kind = 'pct'; min = 10; max = 200; step = 10; dflt = 50; deftext = 'Default (50%)'; label = 'Mouse speed'; help = 'How fast the mouse turns the camera (Static and Smooth camera), in percent.' },
    @{ g = 'play'; key = 'strafe'; kind = 'choice'; def = 'on'; label = 'Strafe aiming'; ch = $OnOff; help = 'Hold the aim button to move sideways while aiming.' },
    @{ g = 'play'; key = 'lockon'; kind = 'choice'; def = 'auto'; label = 'Lock-on circle'; ch = @(@('auto', 'Auto'), @('on', 'On'), @('off', 'Off')); help = 'The red circle that locks onto a target. Auto shows it with a controller and hides it with keyboard and mouse.' },
    @{ g = 'play'; key = 'buyall'; kind = 'choice'; def = 'on'; label = 'Buy all ammo'; ch = $OnOff; help = 'A shortcut at the Gadgetron vendor that fills up your ammo in one press.' },
    @{ g = 'play'; key = 'intro'; kind = 'choice'; def = 'play'; label = 'Skip intro'; ch = @(@('skip', 'On'), @('play', 'Off')); help = 'Starts the game without the opening logos and movie, straight to the title screen. Off plays them as on the console.' },
    @{ g = 'play'; key = 'skipcut'; kind = 'choice'; def = 'on'; label = 'Start skips cutscenes'; ch = $OnOff; help = 'On: Start (Enter or Esc, or the controller''s Start) ends a movie, a scene or a ship flight. Off: the original rule (Start alone only in some places, otherwise with L1 + R1 + L2 + R2 held).' },
    @{ g = 'lang'; key = 'lang'; kind = 'choice'; def = ''; label = 'Language'; ch = @(@('en', 'English'), @('fr', ('Fran' + [char]0xE7 + 'ais')), @('de', 'Deutsch'), @('es', ('Espa' + [char]0xF1 + 'ol')), @('it', 'Italiano')); help = 'Leave it unmarked and the game asks at the title screen, as on the console.' },
    @{ g = 'adv'; key = 'culling'; kind = 'choice'; def = 'on'; label = 'Culling'; ch = $OnOff; help = 'On skips drawing what is out of view. Off draws everything, which only costs speed. Every graphics preset sets it On.' },
    @{ g = 'adv'; key = 'debug'; kind = 'choice'; def = 'off'; label = 'Debug mode'; ch = @(@('off', 'Off'), @('on', 'On')); help = 'The test keys: F1 planet list and warp, F2 free camera, F3 draw distance, F5 immortality, F6 infinite ammo, F8 +10000 bolts. Off for normal play.' }
)
$SetGroups = @(
    @{ g = 'gfx'; title = 'Graphics'; note = '' },
    @{ g = 'disp'; title = 'Display'; note = '' },
    @{ g = 'enh'; title = 'Enhanced picture'; note = 'These effects are part of the Enhanced picture. Set Lighting to Enhanced above to use them.' },
    @{ g = 'play'; title = 'Gameplay'; note = '' },
    @{ g = 'lang'; title = 'Language'; note = '' },
    @{ g = 'adv'; title = 'Advanced'; note = '' }
)
function Get-RowDef([string]$key) { foreach ($r in $SetRows) { if ($r.key -eq $key) { return $r } }; return $null }
function Get-RowDefault([string]$key) { $r = Get-RowDef $key; if ($r) { return $r.def }; return $null }
function Get-CameraByte {
    $modern = ((Get-Opt 'camera') -ne 'classic'); $smooth = ((Get-Opt 'camfeel') -ne 'static')
    if (-not $modern) { return 0 }
    if (-not $smooth) { return 1 }
    return 2
}
function Set-CameraByte([int]$b) {
    # as the game's menu does: Classic keeps the remembered feel; only Static is remembered (Smooth is the default and leaves no key)
    Set-OptRaw 'camera' $(if ($b -ne 0) { 'modern' } else { 'classic' })
    if ($b -ne 0) { Set-OptRaw 'camfeel' $(if ($b -lt 2) { 'static' } else { $null }) }
}
function Get-RowValue($row) {
    if ($row.extra) { return (Get-ExtraValue $row.key) }
    $v = Get-Opt $row.key
    if ($null -eq $v) { return $row.def }
    return $v
}
function Get-RowIndex($row) {
    if ($row.kind -eq 'camera') { return (Get-CameraByte) }
    $v = Get-RowValue $row
    for ($i = 0; $i -lt $row.ch.Count; $i++) { if ($row.ch[$i][0] -eq $v) { return $i } }
    return -1
}
function Get-PctValue($row) {
    # the percent the player set; $null = the row's default
    if ($row.extra) { $v = Get-ExtraValue $row.key; if ($v) { return [int]$v }; return $null }
    $v = Get-Opt $row.key
    if ($null -eq $v) { return $null }
    return [int]$v
}

# --- graphics presets (rac-rs\assets\gfx_presets.txt; this copy only when the file is missing or broken)
$GfxPresetKeys = @('lighting', 'draw', 'smoothing', 'hud', 'ao', 'shadows', 'water', 'particles', 'sunrays', 'vegetation', 'gi', 'waves', 'fog', 'clouds', 'texfilter')
$GfxPresetBuiltIn = @{
    low = @('classic', 'low', 'off', 'original', 'low', 'low', 'low', 'low', 'low', 'off', 'off', 'off', 'classic', 'classic', 'smooth')
    medium = @('classic', 'low', 'on', 'sharp', 'medium', 'medium', 'medium', 'medium', 'medium', 'moderate', 'off', 'on', 'low', 'low', 'smooth')
    high = @('classic', 'medium', 'on', 'sharp', 'high', 'high', 'high', 'high', 'high', 'moderate', 'off', 'on', 'high', 'high', 'smooth')
}
function Get-GfxPresets {
    if ($S.presets) { return $S.presets }
    $t = @{}
    foreach ($n in @('low', 'medium', 'high')) { $t[$n] = @{}; for ($i = 0; $i -lt $GfxPresetKeys.Count; $i++) { $t[$n][$GfxPresetKeys[$i]] = $GfxPresetBuiltIn[$n][$i] } }
    $f = Join-Path $root 'rac-rs\assets\gfx_presets.txt'
    try {
        if (Test-Path -LiteralPath $f -PathType Leaf) {
            $file = @{ low = @{}; medium = @{}; high = @{} }
            foreach ($line in [System.IO.File]::ReadAllLines($f)) {
                $l = ($line -replace '#.*$', '').Trim()
                if ($l -match '^(low|medium|high)\.([a-z_]+)\s*=\s*(\S+)$') { $c = ConvertTo-OptCanon $Matches[2] $Matches[3]; if ($c) { $file[$Matches[1]][$Matches[2]] = $c } }
            }
            $ok = $true
            foreach ($n in @('low', 'medium', 'high')) { foreach ($k in $GfxPresetKeys) { if (-not $file[$n].ContainsKey($k)) { $ok = $false } } }
            if ($ok) { $t = $file }
        }
    } catch { }
    $S.presets = $t
    return $t
}
function Set-GfxPreset([string]$n) {
    # the preset's values (one equal to the game's default removes its key, as the RAC-RS Launcher does), and Culling On
    $t = (Get-GfxPresets)[$n]
    foreach ($k in $GfxPresetKeys) { $v = $t[$k]; Set-OptRaw $k $(if ($v -eq (Get-RowDefault $k)) { $null } else { $v }) }
    Set-OptRaw 'culling' $null
}
function Get-GfxPresetIndex {
    # 0 Low, 1 Medium, 2 High (High first: the game's default), 3 Custom; Culling Off matches no preset
    if ((Get-Opt 'culling') -eq 'off') { return 3 }
    $t = Get-GfxPresets
    foreach ($p in @(@('high', 2), @('medium', 1), @('low', 0))) {
        $all = $true
        foreach ($k in $GfxPresetKeys) {
            $cur = Get-Opt $k
            if ($null -eq $cur) { $cur = Get-RowDefault $k }
            if ($cur -ne $t[$p[0]][$k]) { $all = $false; break }
        }
        if ($all) { return $p[1] }
    }
    return 3
}

# --- building the Settings page
function Add-RowLayout($card, [int]$y, [string]$label, [string]$help, $ctl, [bool]$muted = $false) {
    # label and help on the left, the control on the right (centred on the text), or under the text when it needs more than half the width
    $pad = Px 14; $inW = $card.Width - 2 * $pad
    $stack = ($ctl.Width -gt [int]($inW * 0.5))
    $lw = $(if ($stack) { $inW } else { $inW - $ctl.Width - (Px 16) })
    $l1 = Add-Text $card $label $pad $y $lw $fRow $(if ($muted) { $cDim } else { $cText })
    $bottom = $l1.Bottom
    $l2 = $null
    if ($help) { $l2 = Add-Text $card $help $pad $bottom $lw $fSmall $(if ($muted) { $cDim } else { $cGrey }); $bottom = $l2.Bottom }
    if ($stack) { $ctl.Location = New-Object System.Drawing.Point($pad, ($bottom + (Px 6))); $bottom = $ctl.Bottom }
    else {
        $ty = $y + [math]::Max(0, [int](($bottom - $y - $ctl.Height) / 2))
        $ctl.Location = New-Object System.Drawing.Point(($pad + $inW - $ctl.Width), $ty)
        $bottom = [math]::Max($bottom, $ctl.Bottom)
    }
    $card.Controls.Add($ctl)
    return @{ bottom = $bottom; label = $l1; help = $l2 }
}
function New-PctControl($ctx, [int]$cardW) {
    $row = $ctx.row
    $p = New-Object System.Windows.Forms.Panel
    $p.BackColor = $cCard
    $p.Size = New-Object System.Drawing.Size((Px 380), (Px 26))
    $start = Get-PctValue $row
    if ($null -eq $start) { $start = $(if ($null -ne $row.dflt) { $row.dflt } else { 50 }) }
    $sl = New-Slider $row.min $row.max $row.step $start (Px 180) { param($c, $v) $c.vlbl.Text = ('{0}%' -f [int]$v); $c.vlbl.ForeColor = $cText } { param($c, $v) Set-RowPct $c ([int]$v) } $ctx
    $sl.Location = New-Object System.Drawing.Point(0, (Px 1))
    $p.Controls.Add($sl)
    $vl = New-Label '' (Px 188) 0 (Px 112) (Px 26) $fSmall $cText
    $vl.TextAlign = 'MiddleLeft'; $vl.BackColor = $cCard
    $p.Controls.Add($vl)
    $ln = New-Link 'Use default' (Px 300) 0 (Px 80) (Px 26)
    $ln.TextAlign = 'MiddleLeft'; $ln.BackColor = $cCard; $ln.Font = $fSmall; $ln.Tag = $ctx
    $ln.Add_LinkClicked({ param($sx, $e) try { Set-RowPct $sx.Tag $null } catch { Show-PageError $_ } })
    $p.Controls.Add($ln)
    $ctx.slider = $sl; $ctx.vlbl = $vl; $ctx.lnk = $ln
    return $p
}
function Update-RowControl($ctx) {
    $row = $ctx.row
    if ($row.kind -eq 'pct') {
        $cur = Get-PctValue $row
        if ($null -ne $cur) { $ctx.vlbl.Text = ('{0}%' -f $cur); $ctx.vlbl.ForeColor = $cText; $ctx.lnk.Visible = $true; $ctx.slider.Tag.value = [double]$cur }
        else { $ctx.vlbl.Text = $row.deftext; $ctx.vlbl.ForeColor = $cGrey; $ctx.lnk.Visible = $false; $ctx.slider.Tag.value = [double]$(if ($null -ne $row.dflt) { $row.dflt } else { 50 }) }
        $ctx.slider.Invalidate()
        return
    }
    $ctx.ctl.Tag.sel = Get-RowIndex $row
    $ctx.ctl.Invalidate()
}
function Add-SettingRow($card, [int]$y, $row, [bool]$muted) {
    $ctx = @{ row = $row; ctl = $null; slider = $null; vlbl = $null; lnk = $null }
    if ($row.kind -eq 'pct') { $ctl = New-PctControl $ctx $card.Width }
    else {
        $labels = [string[]]@($row.ch | ForEach-Object { $_[1] })
        $ctl = New-Segmented $labels (Get-RowIndex $row) { param($c, $i) Set-RowChoice $c $i } $ctx
        $ctl.Tag.muted = $muted
    }
    $ctx.ctl = $ctl
    $r = Add-RowLayout $card $y $row.label $row.help $ctl $muted
    $S.setCtl += $ctx
    Update-RowControl $ctx
    return $r.bottom
}
function Set-RowChoice($ctx, [int]$i) {
    if (-not (Confirm-GameClosed)) { return }
    $row = $ctx.row
    if ($row.kind -eq 'camera') { Set-CameraByte $i }
    elseif ($row.extra) { Set-OptRaw $row.key $row.ch[$i][0] }
    else { Set-OptRaw $row.key $row.ch[$i][0] }
    if (-not (Save-PcOptions)) { return }
    Set-PageNote $PageView.settings 'Saved. The game uses it the next time it starts.' $cGreen
    if ($row.key -eq 'lighting') { Request-PageBuild 'settings'; return }      # the Enhanced picture rows change their look
    Update-SettingsState
}
function Set-RowPct($ctx, $v) {
    if (-not (Confirm-GameClosed)) { Update-RowControl $ctx; return }
    $row = $ctx.row
    if ($null -eq $v) { Set-OptRaw $row.key $null }
    elseif ($row.extra) { Set-OptRaw $row.key $(if ($v -eq $row.dflt) { $null } else { [string]$v }) }
    else { Set-OptRaw $row.key ([string](Get-SnapPct $v)) }
    if (Save-PcOptions) { Set-PageNote $PageView.settings 'Saved. The game uses it the next time it starts.' $cGreen }
    Update-SettingsState
}
function Update-SettingsState {
    foreach ($c in $S.setCtl) { Update-RowControl $c }
    if ($S.presetCtl) {
        $pi = Get-GfxPresetIndex
        $S.presetCtl.Tag.sel = $pi
        $S.presetCtl.Tag.locked = @($false, $false, $false, ($pi -ne 3))
        $S.presetCtl.Invalidate()
    }
}
function Add-SettingsPresetCard($c, [int]$y) {
    $y = Add-GroupTitle $c $y 'Graphics quality'
    $card = New-Card $c $y
    $pi = Get-GfxPresetIndex
    $seg = New-Segmented ([string[]]@('Low', 'Medium', 'High', 'Custom')) $pi {
        param($ctx, $i)
        if ($i -gt 2 -or -not (Confirm-GameClosed)) { return }
        Set-GfxPreset (@('low', 'medium', 'high')[$i])
        if (Save-PcOptions) { Set-PageNote $PageView.settings ('Graphics set to ' + @('Low', 'Medium', 'High')[$i] + '. The game uses it the next time it starts.') $cGreen }
        Request-PageBuild 'settings'
    } $null
    $seg.Tag.locked = @($false, $false, $false, ($pi -ne 3))      # Custom is a status, not a button
    $S.presetCtl = $seg
    $r = Add-RowLayout $card (Px 12) 'Preset' 'Sets the picture options below in one click. Low and Medium turn the costly ones down for slower PCs; High is the game''s own default. Change any option afterwards and this shows Custom.' $seg
    $card.Height = $r.bottom + (Px 12)
    return ($card.Bottom + (Px 10))
}
function Add-SettingsGroup($c, [int]$y, $grp) {
    $y = Add-GroupTitle $c $y $grp.title
    $card = New-Card $c $y
    $cy = Px 12
    $muted = $false
    if ($grp.g -eq 'enh' -and (Get-Opt 'lighting') -ne 'enhanced') {
        $muted = $true
        $n = Add-Text $card $grp.note (Px 14) $cy ($card.Width - (Px 28)) $fSmall $cAccent
        $cy = $n.Bottom + (Px 8)
    }
    $rows = @($SetRows | Where-Object { $_.g -eq $grp.g })
    for ($i = 0; $i -lt $rows.Count; $i++) {
        if ($i -gt 0) { Add-Line $card (Px 14) ($cy + (Px 7)) ($card.Width - (Px 28)); $cy += Px 15 }
        $cy = Add-SettingRow $card $cy $rows[$i] $muted
    }
    if ($grp.g -eq 'play') {
        $cy += Px 10
        $n = Add-Text $card 'Keyboard keys and controller buttons are changed in the game: Pause > Options > Controls.' (Px 14) $cy ($card.Width - (Px 28)) $fSmall $cGrey
        $cy = $n.Bottom
    }
    if ($grp.g -eq 'adv') { $cy = Add-SettingsTools $card $cy }
    $card.Height = $cy + (Px 12)
    return ($card.Bottom + (Px 10))
}
function Add-SettingsTools($card, [int]$cy) {
    $pad = Px 14; $inW = $card.Width - 2 * $pad
    Add-Line $card $pad ($cy + (Px 7)) $inW; $cy += Px 15
    $l = Add-Text $card 'Files' $pad $cy $inW $fRow $cText
    $cy = $l.Bottom + (Px 4)
    $x = $pad
    foreach ($b in @(@('Open game log', 'log'), @('Open saves and settings folder', 'player'), @('Open game folder', 'root'))) {
        $w = (Measure-TextW $b[0] $fN) + (Px 26)
        if ($x + $w -gt $pad + $inW) { $x = $pad; $cy += Px 34 }
        $btn = Add-PageButton $card $b[0] $x $cy $b[1] {
            param($sx, $e)
            $pl = Get-PlayerDir
            if ($sx.Tag -eq 'log') {
                $f = Join-Path $pl 'log.txt'
                if (Test-Path -LiteralPath $f -PathType Leaf) { try { Start-Process -FilePath $f } catch { Open-Path $pl } }
                else { Set-PageNote $PageView.settings 'There is no log yet. It is made the first time you play.' $cGrey }
            } elseif ($sx.Tag -eq 'player') { if (-not (Test-Path -LiteralPath $pl)) { [void][System.IO.Directory]::CreateDirectory($pl) }; Open-Path $pl }
            else { Open-Path $root }
        }
        $x = $btn.Right + (Px 8)
    }
    $cy += Px 36
    Add-Line $card $pad ($cy + (Px 3)) $inW; $cy += Px 11
    $b = Add-PageButton $card 'Reset to defaults...' 0 0 $null {
        param($sx, $e)
        if (-not (Confirm-GameClosed)) { return }
        if (-not $S.test) {
            $r = [System.Windows.Forms.MessageBox]::Show($form, "Put every option on this page back to the game's own default?`r`n`r`nYour saves, your keyboard and controller buttons and your mods are not touched.", $AppTitle, 'YesNo', 'Question')
            if ($r -ne [System.Windows.Forms.DialogResult]::Yes) { return }
        }
        Reset-PcOptions
    }
    $r = Add-RowLayout $card $cy 'Reset settings' "Puts every option above back to the game's own default. Your saves, your keyboard and controller buttons and your mods are not touched." $b
    return $r.bottom
}
function Reset-PcOptions {
    foreach ($row in $SetRows) { Set-OptRaw $row.key $null }
    Set-OptRaw 'camfeel' $null
    if (Save-PcOptions) { Set-PageNote $PageView.settings 'Every option is back to the default.' $cGreen }
    Request-PageBuild 'settings'
}
function Build-SettingsPage {
    $pg = $PageView.settings
    $S.opt = Read-PcOptions
    $S.setCtl = @(); $S.presetCtl = $null
    $c = New-PageContent $pg
    $c.SuspendLayout()
    $y = 0
    $y = Add-SettingsPresetCard $c $y
    foreach ($grp in $SetGroups) { $y = Add-SettingsGroup $c $y $grp }
    $c.Height = $y
    $c.ResumeLayout()
    Set-PageContent $pg $c
    Update-SettingsState
}
function Open-SettingsPage {
    $pg = $PageView.settings
    $S.presets = $null
    Set-PageNote $pg '' $null
    $pg.sub.Text = $pg.subText; $pg.sub.ForeColor = $cGrey
    if (Test-GameRunning) { $pg.sub.Text = 'The game is running. Close it to change these: it keeps its own copy while it runs.'; $pg.sub.ForeColor = $cAccent }
    Build-SettingsPage
    $pg.scroll.AutoScrollPosition = New-Object System.Drawing.Point(0, 0)
    try { $pg.scroll.Focus() | Out-Null } catch { }
}

# ================================================================ JSON (settings.json, a pack's mod.json): read with ConvertFrom-Json, kept
# as ordered dictionaries, written with 2-space indents like the game and the RAC-RS Launcher (a number that is a float keeps a decimal point)
function ConvertTo-JTree($v) {
    if ($v -is [System.Management.Automation.PSCustomObject]) {
        $o = [ordered]@{}
        foreach ($p in $v.PSObject.Properties) { $o[$p.Name] = ConvertTo-JTree $p.Value }
        return $o
    }
    if ($v -is [array]) {
        $l = New-Object System.Collections.ArrayList
        foreach ($x in $v) { [void]$l.Add((ConvertTo-JTree $x)) }
        return , $l
    }
    return $v
}
function ConvertTo-JsonStr([string]$sx) {
    $sb = New-Object System.Text.StringBuilder
    [void]$sb.Append('"')
    foreach ($ch in $sx.ToCharArray()) {
        $n = [int]$ch
        if ($n -eq 34) { [void]$sb.Append('\"') }
        elseif ($n -eq 92) { [void]$sb.Append('\\') }
        elseif ($n -eq 10) { [void]$sb.Append('\n') }
        elseif ($n -eq 13) { [void]$sb.Append('\r') }
        elseif ($n -eq 9) { [void]$sb.Append('\t') }
        elseif ($n -lt 32) { [void]$sb.Append(('\u{0:x4}' -f $n)) }
        else { [void]$sb.Append($ch) }
    }
    [void]$sb.Append('"')
    return $sb.ToString()
}
function ConvertTo-JsonText($v, [string]$ind = '') {
    if ($null -eq $v) { return 'null' }
    if ($v -is [bool]) { if ($v) { return 'true' }; return 'false' }
    if ($v -is [string] -or $v -is [char]) { return (ConvertTo-JsonStr ([string]$v)) }
    if ($v -is [int] -or $v -is [long] -or $v -is [int16] -or $v -is [byte] -or $v -is [uint32] -or $v -is [uint64]) { return ([string]$v) }
    if ($v -is [double] -or $v -is [single] -or $v -is [decimal]) {
        $d = [double]$v
        if ([double]::IsNaN($d) -or [double]::IsInfinity($d)) { return 'null' }
        $sx = $d.ToString('R', [System.Globalization.CultureInfo]::InvariantCulture)
        if ($sx -notmatch '[.eE]') { $sx += '.0' }
        return $sx
    }
    $in2 = $ind + '  '
    if ($v -is [System.Management.Automation.PSCustomObject]) { $v = ConvertTo-JTree $v }
    if ($v -is [System.Collections.IDictionary]) {
        if ($v.get_Count() -eq 0) { return '{}' }
        $parts = New-Object System.Collections.Generic.List[string]
        foreach ($k in @($v.get_Keys())) { $parts.Add($in2 + (ConvertTo-JsonStr ([string]$k)) + ': ' + (ConvertTo-JsonText $v[$k] $in2)) }
        return "{`n" + ($parts -join ",`n") + "`n" + $ind + '}'
    }
    if ($v -is [System.Collections.IEnumerable]) {
        $parts = New-Object System.Collections.Generic.List[string]
        foreach ($x in $v) { $parts.Add($in2 + (ConvertTo-JsonText $x $in2)) }
        if ($parts.Count -eq 0) { return '[]' }
        return "[`n" + ($parts -join ",`n") + "`n" + $ind + ']'
    }
    return (ConvertTo-JsonStr ([string]$v))
}
function Read-JsonText([string]$text) {
    # @{ ok; value (an ordered tree); err }
    try { $j = $text.TrimStart([char]0xFEFF) | ConvertFrom-Json } catch { return @{ ok = $false; err = ($_.Exception.Message -split "`r?`n")[0] } }
    return @{ ok = $true; value = (ConvertTo-JTree $j); obj = ($j -is [System.Management.Automation.PSCustomObject]) }
}
function Test-JInt($v) { return ($v -is [int] -or $v -is [long]) }
function Test-JNum($v) { return ($v -is [int] -or $v -is [long] -or $v -is [decimal] -or $v -is [double]) }
function Test-JDict($v) { return ($v -is [System.Collections.IDictionary]) }
function Test-JList($v) { return ($v -is [System.Collections.IList] -and -not ($v -is [string])) }
function Get-JVal($o, [string]$k) { if ((Test-JDict $o) -and $o.Contains($k)) { return , $o[$k] }; return $null }

# ================================================================ script mods: omni-mod.json and its settings (omni\modset.rs; the RAC-RS
# Launcher's mods\script.rs and settings.rs: the loader's rules, so a mod the game would refuse shows its problem and cannot be switched on)
$BindNames = @('KeyA', 'KeyB', 'KeyC', 'KeyD', 'KeyE', 'KeyF', 'KeyG', 'KeyH', 'KeyI', 'KeyJ', 'KeyK', 'KeyL', 'KeyM', 'KeyN', 'KeyO', 'KeyP', 'KeyQ', 'KeyR', 'KeyS', 'KeyT', 'KeyU', 'KeyV', 'KeyW', 'KeyX', 'KeyY', 'KeyZ',
    'Digit1', 'Digit2', 'Digit3', 'Digit4', 'Digit5', 'Digit6', 'Digit7', 'Digit8', 'Digit9', 'Digit0', 'Enter', 'Escape', 'Backspace', 'Tab', 'Space', 'Minus', 'Equal', 'BracketLeft', 'BracketRight',
    'Backslash', 'IntlHash', 'Semicolon', 'Quote', 'Backquote', 'Comma', 'Period', 'Slash', 'CapsLock', 'F1', 'F2', 'F3', 'F4', 'F5', 'F6', 'F7', 'F8', 'F9', 'F10', 'F11', 'F12', 'PrintScreen', 'ScrollLock', 'Pause',
    'Insert', 'Home', 'PageUp', 'Delete', 'End', 'PageDown', 'ArrowRight', 'ArrowLeft', 'ArrowDown', 'ArrowUp', 'NumLock', 'NumpadDivide', 'NumpadMultiply', 'NumpadSubtract', 'NumpadAdd', 'NumpadEnter',
    'Numpad1', 'Numpad2', 'Numpad3', 'Numpad4', 'Numpad5', 'Numpad6', 'Numpad7', 'Numpad8', 'Numpad9', 'Numpad0', 'NumpadDecimal', 'IntlBackslash', 'ContextMenu', 'NumpadEqual', 'ControlLeft', 'ShiftLeft',
    'AltLeft', 'MetaLeft', 'ControlRight', 'ShiftRight', 'AltRight', 'MetaRight', 'MouseLeft', 'MouseRight', 'MouseMiddle', 'MouseBack', 'MouseForward', 'WheelUp', 'WheelDown', 'WheelLeft', 'WheelRight')
function Get-BindList([string]$v) { return , @($v -split '[,\s]+' | Where-Object { $_ }) }
function Get-BindProblem([string]$v, [int]$most) {
    $names = Get-BindList $v
    $bad = @($names | Where-Object { $BindNames -cnotcontains $_ })
    if ($bad.Count -gt 0) { return ('names no key, button or wheel direction: ' + ($bad -join ', ')) }
    if (@($names | Select-Object -Unique).Count -ne $names.Count) { return 'names one input twice' }
    if ($names.Count -gt $most) { return "has $($names.Count) binds, more than $most" }
    return $null
}
function Get-BindFriendly([string]$n) {
    if ($n -cmatch '^Key([A-Z])$') { return $Matches[1] }
    if ($n -cmatch '^Digit(\d)$') { return $Matches[1] }
    if ($n -cmatch '^Numpad(.+)$') { $m = @{ Divide = '/'; Multiply = '*'; Subtract = '-'; Add = '+'; Decimal = '.'; Equal = '=' }; $x = $Matches[1]; if ($m.ContainsKey($x)) { $x = $m[$x] }; return "Numpad $x" }
    if ($n -cmatch '^Arrow(.+)$') { return "$($Matches[1]) arrow" }
    $t = @{ MouseLeft = 'Left mouse button'; MouseRight = 'Right mouse button'; MouseMiddle = 'Middle mouse button'; MouseBack = 'Mouse back button'; MouseForward = 'Mouse forward button'
        WheelUp = 'Wheel up'; WheelDown = 'Wheel down'; WheelLeft = 'Wheel left'; WheelRight = 'Wheel right'; ShiftLeft = 'Left Shift'; ShiftRight = 'Right Shift'; ControlLeft = 'Left Ctrl'; ControlRight = 'Right Ctrl'
        AltLeft = 'Left Alt'; AltRight = 'Right Alt'; BracketLeft = '['; BracketRight = ']'; Minus = '-'; Equal = '='; Comma = ','; Period = '.'; Slash = '/'; Backslash = '\'; Semicolon = ';'; Quote = "'"; Backquote = '`' }
    if ($t.ContainsKey($n)) { return $t[$n] }
    return $n
}
function Get-BindNameOfKey([System.Windows.Forms.Keys]$k) {
    $sx = $k.ToString()
    if ($sx -cmatch '^[A-Z]$') { return "Key$sx" }
    if ($sx -cmatch '^D(\d)$') { return "Digit$($Matches[1])" }
    if ($sx -cmatch '^NumPad(\d)$') { return "Numpad$($Matches[1])" }
    if ($sx -cmatch '^F(\d{1,2})$' -and [int]$Matches[1] -le 12) { return $sx }
    $m = @{ Return = 'Enter'; Back = 'Backspace'; Tab = 'Tab'; Space = 'Space'; OemMinus = 'Minus'; Oemplus = 'Equal'; OemOpenBrackets = 'BracketLeft'; Oem6 = 'BracketRight'; Oem5 = 'Backslash'; Oem1 = 'Semicolon'
        Oem7 = 'Quote'; Oem3 = 'Backquote'; Oemcomma = 'Comma'; OemPeriod = 'Period'; Oem2 = 'Slash'; Oem102 = 'IntlBackslash'; Capital = 'CapsLock'; Scroll = 'ScrollLock'; Pause = 'Pause'; Insert = 'Insert'
        Home = 'Home'; PageUp = 'PageUp'; Delete = 'Delete'; End = 'End'; Next = 'PageDown'; Right = 'ArrowRight'; Left = 'ArrowLeft'; Down = 'ArrowDown'; Up = 'ArrowUp'; NumLock = 'NumLock'
        Divide = 'NumpadDivide'; Multiply = 'NumpadMultiply'; Subtract = 'NumpadSubtract'; Add = 'NumpadAdd'; Decimal = 'NumpadDecimal'; Apps = 'ContextMenu'; ShiftKey = 'ShiftLeft'; ControlKey = 'ControlLeft'
        Menu = 'AltLeft'; LWin = 'MetaLeft'; RWin = 'MetaRight'; PrintScreen = 'PrintScreen' }
    if ($m.ContainsKey($sx)) { return $m[$sx] }
    return $null
}
function Read-SettingDef([int]$i, $j, $errs) {
    $w = "settings[$i]"
    if (-not (Test-JDict $j)) { $errs.Add("${w}: not an object"); return $null }
    $key = Get-JVal $j 'key'
    if ($key -is [string] -and $key -cmatch '^[a-z0-9_]+$') { $w = "setting $key" } else { $errs.Add("${w}: key must be lower case letters, digits and _"); $key = $null }
    $name = Get-JVal $j 'name'
    if (-not ($name -is [string] -and $name)) { $errs.Add("${w}: no name"); $name = $null }
    $desc = ''
    $dv = Get-JVal $j 'description'
    if ($null -ne $dv) { if ($dv -is [string]) { $desc = $dv } else { $errs.Add("${w}: description is not text") } }
    $type = Get-JVal $j 'type'
    if (-not ($type -is [string] -and $type -cin @('bool', 'int', 'float', 'choice', 'string', 'keybind'))) { $errs.Add("${w}: type must be one of bool, choice, float, int, keybind, string"); return $null }
    if (-not $j.Contains('default')) { $errs.Add("${w}: no default"); return $null }
    $d = $j['default']
    $n0 = $errs.Count
    $def = @{ key = $key; name = $name; desc = $desc; type = $type; default = $d; advanced = $false; min = $null; max = $null; step = $null; choices = @(); maxlen = $null; binds = 2; group = '' }
    switch ($type) {
        'bool' { if ($d -isnot [bool]) { $errs.Add("${w}: the default is not true or false") } }
        { $_ -in @('int', 'float') } {
            $int = ($type -eq 'int')
            if (-not ((Test-JNum $d) -and (-not $int -or (Test-JInt $d)))) { $errs.Add("${w}: the default is not $(if ($int) { 'an integer' } else { 'a number' })"); break }
            foreach ($f in @('min', 'max')) {
                $x = Get-JVal $j $f
                if ($null -eq $x) { continue }
                if ((Test-JNum $x) -and (-not $int -or (Test-JInt $x))) { $def[$f] = [double]$x } else { $errs.Add("${w}: $f is not $(if ($int) { 'an integer' } else { 'a number' })") }
            }
            if ($null -ne $def.min -and $null -ne $def.max -and $def.min -gt $def.max) { $errs.Add("${w}: min is above max") }
            if (($null -ne $def.min -and [double]$d -lt $def.min) -or ($null -ne $def.max -and [double]$d -gt $def.max)) { $errs.Add("${w}: the default is out of range") }
            if (-not $int) { $st = Get-JVal $j 'step'; if ($null -ne $st) { if ((Test-JNum $st) -and [double]$st -gt 0) { $def.step = [double]$st } else { $errs.Add("${w}: step must be a positive number") } } }
        }
        'choice' {
            $list = Get-JVal $j 'choices'
            if (-not (Test-JList $list) -or $list.Count -lt 2) { $errs.Add("${w}: a choice needs two choices or more"); break }
            $ch = New-Object System.Collections.ArrayList
            foreach ($c in $list) {
                $cv = Get-JVal $c 'value'; $cn = Get-JVal $c 'name'
                if (-not ($cv -is [string] -and $cn -is [string])) { $errs.Add("${w}: each choice is {value, name}, both text"); break }
                [void]$ch.Add(@{ value = $cv; name = $cn })
            }
            if ($errs.Count -ne $n0) { break }
            if (@($ch | ForEach-Object { $_.value } | Select-Object -Unique).Count -ne $ch.Count) { $errs.Add("${w}: a value twice among the choices") }
            if (-not ($d -is [string] -and @($ch | Where-Object { $_.value -ceq $d }).Count -gt 0)) { $errs.Add("${w}: the default is not one of the choices") }
            $def.choices = $ch.ToArray()
        }
        'keybind' {
            $b = Get-JVal $j 'binds'
            if ($null -ne $b) { if ((Test-JInt $b) -and $b -in @(1, 2)) { $def.binds = [int]$b } else { $errs.Add("${w}: binds must be 1 or 2"); break } }
            if ($d -isnot [string]) { $errs.Add("${w}: the default is not text"); break }
            $why = Get-BindProblem $d $def.binds
            if ($why) { $errs.Add("${w}: the default $why") }
            $g = Get-JVal $j 'group'; if ($g -is [string]) { $def.group = $g }
        }
        'string' {
            if ($d -isnot [string]) { $errs.Add("${w}: the default is not text"); break }
            $ml = Get-JVal $j 'max_length'
            if ($null -ne $ml) { if ((Test-JInt $ml) -and $ml -gt 0) { $def.maxlen = [int]$ml } else { $errs.Add("${w}: max_length must be a positive integer"); break } }
            if ($def.maxlen -and $d.Length -gt $def.maxlen) { $errs.Add("${w}: the default is longer than max_length") }
        }
    }
    if ($errs.Count -ne $n0 -or -not $key -or -not $name) { return $null }
    if ($type -eq 'float') { $def.default = [double]$d }
    $def.advanced = ($type -ne 'keybind' -and (Get-JVal $j 'group') -eq 'advanced')
    return $def
}
function Get-ValueProblem($d, $v) {
    switch ($d.type) {
        'bool' { if ($v -isnot [bool]) { return 'is not true or false' } }
        'int' { if (-not (Test-JInt $v)) { return 'is not an integer' }; if (($null -ne $d.min -and $v -lt $d.min) -or ($null -ne $d.max -and $v -gt $d.max)) { return 'is out of range' } }
        'float' { if (-not (Test-JNum $v)) { return 'is not a number' }; if (($null -ne $d.min -and [double]$v -lt $d.min) -or ($null -ne $d.max -and [double]$v -gt $d.max)) { return 'is out of range' } }
        'choice' { if (-not ($v -is [string] -and @($d.choices | Where-Object { $_.value -ceq $v }).Count -gt 0)) { return 'is not one of the choices' } }
        'string' { if ($v -isnot [string]) { return 'is not text' }; if ($d.maxlen -and $v.Length -gt $d.maxlen) { return 'is longer than max_length' } }
        'keybind' { if ($v -isnot [string]) { return 'is not text' }; return (Get-BindProblem $v $d.binds) }
    }
    return $null
}
function Read-ScriptManifest([string]$text) {
    # @{ state = 'mod' | 'data' (format 1: a data pack, left to its mod.json) | 'bad'; err; m }
    $r = Read-JsonText $text
    if (-not $r.ok) { return @{ state = 'bad'; err = "it is not valid JSON ($($r.err))" } }
    $o = $r.value
    if (-not (Test-JDict $o)) { return @{ state = 'bad'; err = 'it is not a JSON object' } }
    $fmt = Get-JVal $o 'format'
    if ((Test-JInt $fmt) -and $fmt -eq 1) { return @{ state = 'data' } }
    if ((Test-JInt $fmt) -and $fmt -gt 2) { return @{ state = 'bad'; err = 'needs a newer version of the launcher' } }
    if (-not ((Test-JInt $fmt) -and $fmt -eq 2)) { return @{ state = 'bad'; err = 'format must be 2' } }
    $errs = New-Object System.Collections.Generic.List[string]
    $id = Get-JVal $o 'id'
    if (-not ($id -is [string] -and $id.Length -le 64 -and $id -cmatch '^[a-z0-9]+(-[a-z0-9]+)*$')) { $errs.Add('id: lower case letters and digits joined by single hyphens, at most 64 characters'); $id = '' }
    $name = Get-JVal $o 'name'
    if (-not ($name -is [string] -and $name.Trim())) { $errs.Add('name: required'); $name = '' }
    $txt = @{}
    foreach ($k in @('version', 'author', 'description')) { $v = Get-JVal $o $k; if ($null -eq $v) { $txt[$k] = '' } elseif ($v -is [string]) { $txt[$k] = $v } else { $errs.Add("${k}: not text"); $txt[$k] = '' } }
    $type = Get-JVal $o 'type'
    $kind = 'native'
    if ($o.Contains('type')) { if ($type -ceq 'script') { $kind = 'script' } elseif ($type -cne 'native') { $errs.Add('type: native (the default) or script') } }
    if ($o.Contains('plugin')) { $kind = 'native' }
    $games = [ordered]@{}
    $gj = Get-JVal $o 'games'
    if (-not (Test-JDict $gj) -or $gj.get_Count() -eq 0) { $errs.Add('games: an object with at least one game') }
    else {
        foreach ($g in @($gj.get_Keys())) {
            $vl = $gj[$g]
            if ((Test-JList $vl) -and $vl.Count -gt 0 -and @($vl | Where-Object { $_ -isnot [string] }).Count -eq 0) { $games[$g] = @($vl) }
            else { $errs.Add("games ${g}: a list of at least one disc version (pal, ntsc)") }
        }
    }
    $settings = New-Object System.Collections.ArrayList
    $sj = Get-JVal $o 'settings'
    if ($null -ne $sj) {
        if (-not (Test-JList $sj)) { $errs.Add('settings: not a list') }
        else {
            for ($i = 0; $i -lt $sj.Count; $i++) { $d = Read-SettingDef $i $sj[$i] $errs; if ($d) { [void]$settings.Add($d) } }
            $keys = @($settings | ForEach-Object { $_.key })
            $dup = @($keys | Group-Object -CaseSensitive | Where-Object { $_.Count -gt 1 } | ForEach-Object { $_.Name })
            if ($dup.Count -gt 0) { $errs.Add("settings: a key twice ($($dup -join ', '))") }
        }
    }
    $presets = New-Object System.Collections.ArrayList
    $pj = Get-JVal $o 'presets'
    if ($null -ne $pj) {
        if (-not (Test-JList $pj)) { $errs.Add('presets: not a list') }
        else {
            for ($i = 0; $i -lt $pj.Count; $i++) {
                $p = $pj[$i]
                $prid = Get-JVal $p 'id'; $pn = Get-JVal $p 'name'; $pv = Get-JVal $p 'values'
                if (-not ($prid -is [string] -and $prid -cmatch '^[a-z0-9_]+$' -and $pn -is [string] -and $pn -and (Test-JDict $pv))) { $errs.Add("presets[$i]: {id (a-z, 0-9, _), name, values}"); continue }
                foreach ($k in @($pv.get_Keys())) {
                    $d = @($settings | Where-Object { $_.key -ceq $k })
                    if ($d.Count -eq 0) { $errs.Add("preset ${prid}: $k is not a setting of the mod") }
                    else { $why = Get-ValueProblem $d[0] $pv[$k]; if ($why) { $errs.Add("preset ${prid}: $k $why") } }
                }
                [void]$presets.Add(@{ id = $prid; name = $pn; values = $pv })
            }
            if (@($presets | ForEach-Object { $_.id } | Select-Object -Unique).Count -ne $presets.Count) { $errs.Add('presets: an id twice') }
        }
    }
    if ($errs.Count -gt 0) {
        $msg = $errs[0]
        if ($errs.Count -eq 2) { $msg += ' (and 1 more problem)' } elseif ($errs.Count -gt 2) { $msg += " (and $($errs.Count - 1) more problems)" }
        return @{ state = 'bad'; err = $msg }
    }
    return @{ state = 'mod'; m = @{ id = $id; name = $name; version = $txt.version; author = $txt.author; description = $txt.description; kind = $kind; games = $games; settings = $settings.ToArray(); presets = $presets.ToArray() } }
}
function Test-SameValue($a, $b) {
    if ((Test-JNum $a) -and (Test-JNum $b)) { return ([math]::Abs([double]$a - [double]$b) -lt 1e-9) }
    if ($a -is [bool] -and $b -is [bool]) { return ($a -eq $b) }
    if ($a -is [string] -and $b -is [string]) { return ($a -ceq $b) }
    return ($null -eq $a -and $null -eq $b)
}
function Get-ModValues($sm, $given) {
    # the values the mod sees: the stored one when it is valid, else the default (the loader's rule)
    $out = @{}
    foreach ($d in $sm.settings) {
        if ((Test-JDict $given) -and $given.Contains($d.key) -and $null -eq (Get-ValueProblem $d $given[$d.key])) {
            $v = $given[$d.key]
            if ($d.type -eq 'float') { $v = [double]$v }
            $out[$d.key] = $v
        } else { $out[$d.key] = $d.default }
    }
    return $out
}
function Get-PresetEffect($sm, $p) {
    # what a preset button writes: every keybind and every setting some preset names, the preset's value, else the default
    $out = New-Object System.Collections.ArrayList
    foreach ($d in $sm.settings) {
        $covered = ($d.type -eq 'keybind')
        foreach ($q in $sm.presets) { if ($q['values'].Contains($d.key)) { $covered = $true } }
        if (-not $covered) { continue }
        $v = $(if ($p['values'].Contains($d.key)) { $p['values'][$d.key] } else { $d.default })
        if ($d.type -eq 'float') { $v = [double]$v }
        [void]$out.Add(@{ key = $d.key; value = $v })
    }
    return , $out
}
function Get-DefaultEffect($sm) {
    $out = New-Object System.Collections.ArrayList
    foreach ($d in $sm.settings) { [void]$out.Add(@{ key = $d.key; value = $d.default }) }
    return , $out
}
function Test-EffectFits($effect, $cur) { foreach ($x in $effect) { if (-not (Test-SameValue $cur[$x.key] $x.value)) { return $false } }; return $true }
function Get-LitPreset($sm, $cur) {
    # the preset whose values the settings have (-1 = the plain Default), $null = custom
    for ($i = 0; $i -lt $sm.presets.Count; $i++) { if (Test-EffectFits (Get-PresetEffect $sm $sm.presets[$i]) $cur) { return $i } }
    if (Test-EffectFits (Get-DefaultEffect $sm) $cur) { return -1 }
    return $null
}

# ================================================================ mods\settings.json (read fresh before every write; everything else kept)
function Read-ModSettingsFile {
    $p = Join-Path (Get-ModsDir) 'settings.json'
    $r = @{ root = [ordered]@{}; newer = $false; broken = $null }
    if (-not (Test-Path -LiteralPath $p -PathType Leaf)) { return $r }
    $text = $null
    try { $text = [System.IO.File]::ReadAllText($p, [System.Text.Encoding]::UTF8) } catch { return $r }
    $j = Read-JsonText $text
    if (-not $j.ok) { $r.broken = $j.err; return $r }
    if (-not (Test-JDict $j.value)) { $r.broken = 'it is not a JSON object'; return $r }
    $r.root = $j.value
    $f = Get-JVal $r.root 'format'
    if ((Test-JInt $f) -and $f -gt 1) { $r.newer = $true }
    return $r
}
function Get-StoredModValues([string]$id) {
    $m = Get-JVal $S.modSet.root 'mods'
    $v = Get-JVal $m $id
    if (Test-JDict $v) { return $v }
    return [ordered]@{}
}
function Set-ModSettingValues([string]$id, $changes) {
    # $changes: @{ key; value; remove }; throws with the reason when nothing could be written
    $f = Read-ModSettingsFile
    if ($f.newer) { throw 'settings.json was written by a newer version of the game; it is left as it is' }
    $p = Join-Path (Get-ModsDir) 'settings.json'
    if ($f.broken) {
        $bad = Join-Path (Get-ModsDir) 'settings.json.broken'
        try { if (Test-Path -LiteralPath $bad) { Remove-Item -LiteralPath $bad -Force }; Move-Item -LiteralPath $p -Destination $bad }
        catch { throw "settings.json is damaged ($($f.broken)) and could not be moved aside: $($_.Exception.Message)" }
    }
    $doc = $f.root
    if ($doc.Contains('format')) { $doc['format'] = 1 } else { $doc.Insert(0, 'format', 1) }
    if (-not ($doc['mods'] -is [System.Collections.Specialized.OrderedDictionary])) { $doc['mods'] = [ordered]@{} }
    $mods = $doc['mods']
    if (-not ($mods[$id] -is [System.Collections.Specialized.OrderedDictionary])) { $mods[$id] = [ordered]@{} }
    $vals = $mods[$id]
    foreach ($c in $changes) {
        # (indexers, not .remove / .value: a hashtable without that key would answer with its own Remove method)
        if ($c['remove']) { if ($vals.Contains($c['key'])) { $vals.Remove($c['key']) } }
        else { $vals[$c['key']] = $c['value'] }
    }
    if ($vals.get_Count() -eq 0) { $mods.Remove($id) }
    Write-FileAtomic $p ((ConvertTo-JsonText $doc '') + "`n")
}

# ================================================================ mods\active.txt (texpack\active.rs, the RAC-RS Launcher's mods\active.rs)
$ActiveHeader = '# Enabled mods, one id per line, highest priority first. Written by the RAC-RS Launcher.'
function Get-ActiveMarker([string]$l) {
    if (-not $l.StartsWith('#')) { return $null }
    $body = $l.Substring(1).Trim()
    $i = $body.IndexOf(':')
    if ($i -lt 0) { return $null }
    $label = $body.Substring(0, $i).Trim(); $val = $body.Substring($i + 1).Trim().ToLowerInvariant()
    $m = $(if ($label -ieq 'Load texture packs') { 'textures' } elseif ($label -ieq 'Load mods') { 'mods' } else { $null })
    if (-not $m) { return $null }
    if ($val -eq 'off') { return @{ m = $m; on = $false } }
    if ($val -eq 'on') { return @{ m = $m; on = $true } }
    return $null
}
function Get-ActiveWord([string]$l) {
    if (-not $l.StartsWith('#')) { return $null }
    $b = $l.Substring(1).Trim()
    if (-not $b -or $b -match '\s' -or $b.StartsWith('#') -or $b.Contains(':')) { return $null }
    return $b
}
function Get-ModMaster($st, [string]$id) {
    # the master tick of an installed mod: 'textures' or 'mods'; $null when it is not installed or cannot be read
    foreach ($e in $st.entries) { if ($e.id -ceq $id) { if (-not $e.ok) { return $null }; if ($e.kind -eq 'textures') { return 'textures' }; return 'mods' } }
    return $null
}
function Read-ActiveFile($st) {
    $p = Join-Path (Get-ModsDir) 'active.txt'
    $text = ''
    if (Test-Path -LiteralPath $p -PathType Leaf) { try { $text = [System.IO.File]::ReadAllText($p, [System.Text.Encoding]::UTF8) } catch { $text = '' } }
    $lines = @(($text -split "`r?`n") | ForEach-Object { $_.TrimStart([char]0xFEFF).Trim() } | Where-Object { $_ })
    foreach ($l in $lines) { $mk = Get-ActiveMarker $l; if ($mk) { $st.off[$mk.m] = (-not $mk.on) } }
    $pending = New-Object System.Collections.Generic.List[string]
    foreach ($l in $lines) {
        if ($l -ceq $ActiveHeader -or (Get-ActiveMarker $l)) { continue }
        $id = $l
        if ($l.StartsWith('#')) {
            $w = Get-ActiveWord $l
            $held = $null
            if ($w) { $mm = Get-ModMaster $st $w; if ($mm -and $st.off[$mm]) { $held = $w } }
            if (-not $held) { $pending.Add($l); continue }
            $id = $held
        }
        foreach ($c in $pending) { [void]$st.kept.Add(@{ anchor = $id; line = $c }) }
        $pending.Clear()
        if (-not $st.active.Contains($id)) { $st.active.Add($id) }
    }
    foreach ($c in $pending) { [void]$st.kept.Add(@{ anchor = $null; line = $c }) }
}
function Format-ActiveFile($st) {
    $sb = New-Object System.Text.StringBuilder
    [void]$sb.Append($ActiveHeader + "`n")
    if ($st.off.textures) { [void]$sb.Append("# Load texture packs: off`n") }
    if ($st.off.mods) { [void]$sb.Append("# Load mods: off`n") }
    $safe = {
        param($c)
        # a typed comment that looks like a held mod would become a switched-on mod at the next tick: ## keeps it a comment
        $w = Get-ActiveWord $c
        if ($w) { $mm = Get-ModMaster $st $w; if ($mm -and $st.off[$mm]) { return ('#' + $c) } }
        return $c
    }
    foreach ($id in $st.active) {
        foreach ($k in $st.kept) { if ($k.anchor -ceq $id) { [void]$sb.Append((& $safe $k.line) + "`n") } }
        $mm = Get-ModMaster $st $id
        if ($mm -and $st.off[$mm]) { [void]$sb.Append('# ') }
        [void]$sb.Append($id + "`n")
    }
    foreach ($k in $st.kept) { if ($null -eq $k.anchor -or -not $st.active.Contains($k.anchor)) { [void]$sb.Append((& $safe $k.line) + "`n") } }
    return $sb.ToString()
}
function Save-ActiveFile {
    try { Write-FileAtomic (Join-Path (Get-ModsDir) 'active.txt') (Format-ActiveFile $S.mods); return $true }
    catch { Set-PageNote $PageView.mods ('The mod list could not be saved: ' + $_.Exception.Message) $cRed; return $false }
}

# ================================================================ the installed mods (the game's texpack\store.rs lists the same ones)
function Test-TexName([string]$fname) {
    # a PCSX2 replacement texture's file name (GSTextureReplacements.cpp): <hash>[-<clut>][-r<W>x<H>]-<bits>.png|dds
    $m = [regex]::Match($fname, '^(.+)\.(png|dds)$', 'IgnoreCase')
    if (-not $m.Success) { return $false }
    $p = $m.Groups[1].Value.Split('-')
    $hex = { param($sx, $n) return ($sx.Length -ge 1 -and $sx.Length -le $n -and $sx -match '^[0-9a-fA-F]+$') }
    $reg = { param($sx) if (-not $sx.StartsWith('r')) { return $false }; $r = $sx.Substring(1); if ($r -match '^\d+x\d+$') { return $true }; return ($r.Length -ge 1 -and $r.Length -le 16 -and $r -match '^[0-9a-fA-F]+$') }
    if ($p.Count -eq 2) { return ((& $hex $p[0] 16) -and (& $hex $p[1] 8)) }
    if ($p.Count -eq 3) { return ((& $hex $p[0] 16) -and ((& $hex $p[1] 16) -or (& $reg $p[1])) -and (& $hex $p[2] 8)) }
    if ($p.Count -eq 4) { return ((& $hex $p[0] 16) -and (& $hex $p[1] 16) -and (& $reg $p[2]) -and (& $hex $p[3] 8)) }
    return $false
}
function Test-BarePack([string]$dir) {
    # a folder with no mod.json and no manifest that the game still reads as a texture pack (texpack\layout.rs, looked at shallowly here)
    if (Test-Path -LiteralPath (Join-Path $dir 'textures') -PathType Container) { return $true }
    $n = 0
    foreach ($f in [System.IO.Directory]::EnumerateFiles($dir, '*', 'AllDirectories')) { if (Test-TexName ([System.IO.Path]::GetFileName($f))) { return $true }; $n++; if ($n -ge 400) { break } }
    return $false
}
function New-ModEntry([string]$folder, [string]$dir) {
    return @{ id = $folder; folder = $folder; dir = $dir; ok = $true; err = ''; name = ''; author = ''; version = ''; desc = ''; kind = 'textures'; game = 'rac1'; format = 1; serials = @(); source = ''; isScript = $false; mfile = ''; sm = $null; bare = $false }
}
function Read-ModStore {
    $st = @{ entries = (New-Object System.Collections.ArrayList); active = (New-Object System.Collections.Generic.List[string]); off = @{ textures = $false; mods = $false }; kept = (New-Object System.Collections.ArrayList) }
    $md = Get-ModsDir
    if (Test-Path -LiteralPath $md -PathType Container) {
        $names = [string[]]@(Get-ChildItem -LiteralPath $md -Directory -Force -ErrorAction SilentlyContinue | ForEach-Object { $_.Name })
        [Array]::Sort($names, [StringComparer]::Ordinal)      # with two folders of one id the first in name order wins, as in the game
        foreach ($f in $names) {
            if ($f.StartsWith('.') -or $f.EndsWith('.tmp')) { continue }
            $dir = Join-Path $md $f
            $mf = $null
            foreach ($x in @('omni-mod.json', 'openrac-mod.json')) { if (-not $mf -and (Test-Path -LiteralPath (Join-Path $dir $x) -PathType Leaf)) { $mf = $x } }
            if ($mf) {
                $e = New-ModEntry $f $dir
                $e.isScript = $true; $e.mfile = $mf; $e.kind = 'script'
                $r = $null
                try { $r = Read-ScriptManifest ([System.IO.File]::ReadAllText((Join-Path $dir $mf), [System.Text.Encoding]::UTF8)) } catch { $r = @{ state = 'bad'; err = $_.Exception.Message } }
                if ($r.state -eq 'mod') {
                    $m = $r.m
                    if (@($st.entries | Where-Object { $_.id -ceq $m.id }).Count -gt 0) { $e.ok = $false; $e.err = "another mod folder already has the id `"$($m.id)`"" }
                    else {
                        $e.id = $m.id; $e.name = $m.name; $e.author = $m.author; $e.version = $m.version; $e.desc = $m.description; $e.kind = $m.kind; $e.sm = $m
                        $e.game = $(if ($m.games.Contains('rac1')) { 'rac1' } else { @($m.games.get_Keys())[0] })
                    }
                    [void]$st.entries.Add($e); continue
                }
                if ($r.state -eq 'bad') { $e.ok = $false; $e.err = $r.err; [void]$st.entries.Add($e); continue }
            }
            $mj = Join-Path $dir 'mod.json'
            if (Test-Path -LiteralPath $mj -PathType Leaf) {
                $e = New-ModEntry $f $dir
                $e.mfile = 'mod.json'
                $r = $null
                try { $r = Read-JsonText ([System.IO.File]::ReadAllText($mj, [System.Text.Encoding]::UTF8)) } catch { $r = @{ ok = $false; err = $_.Exception.Message } }
                if (-not $r.ok) { $e.ok = $false; $e.err = $r.err }
                elseif (-not (Test-JDict $r.value)) { $e.ok = $false; $e.err = 'it is not a JSON object' }
                else {
                    $o = $r.value
                    foreach ($k in @('name', 'author', 'version', 'description', 'source')) { $v = Get-JVal $o $k; if ($v -is [string]) { $e[$(if ($k -eq 'description') { 'desc' } else { $k })] = $v } }
                    $v = Get-JVal $o 'game'; if ($v -is [string]) { $e.game = $v }
                    $v = Get-JVal $o 'kind'; if ($v -is [string]) { $e.kind = $v }
                    $v = Get-JVal $o 'format'; if ($null -ne $v) { if (Test-JInt $v) { $e.format = [int]$v } else { $e.format = -1 } }
                    $v = Get-JVal $o 'serials'; if (Test-JList $v) { $e.serials = @($v | Where-Object { $_ -is [string] }) }
                }
                [void]$st.entries.Add($e); continue
            }
            $bare = $false
            try { $bare = Test-BarePack $dir } catch { $bare = $false }
            if ($bare) {
                $e = New-ModEntry $f $dir
                $e.bare = $true; $e.desc = 'A texture pack without a mod.json. The game reads it as a texture pack.'
                [void]$st.entries.Add($e)
            }
        }
    }
    $sorted = @($st.entries | Sort-Object -Property @{ Expression = { (Get-ModName $_).ToLowerInvariant() } }, @{ Expression = { $_.id } })
    $st.entries = New-Object System.Collections.ArrayList
    foreach ($e in $sorted) { [void]$st.entries.Add($e) }
    Read-ActiveFile $st
    return $st
}
function Get-ModName($e) { if ($e.name) { return $e.name }; return $e.id }
function Get-ModEntry([string]$id) { foreach ($e in $S.mods.entries) { if ($e.id -ceq $id) { return $e } }; return $null }
function Get-ModBlock($e) {
    # why a mod cannot be switched on ($null = it can)
    if (-not $e.ok) { if ($e.isScript) { return "$($e.mfile) has a problem: $($e.err)" }; return "mod.json could not be read ($($e.err))" }
    if ($e.format -ne 1) { return 'needs a newer version of the launcher' }
    if ($e.kind -notin @('textures', 'script', 'native')) { return 'needs a newer version' }
    if ($e.game -ne 'rac1' -and -not ($e.sm -and $e.sm.games.Contains('rac1'))) { return "made for another game ($($e.game))" }
    return $null
}
function Get-DiscSerial {
    # the installed disc's serial, by its size ($null when it is not known)
    try {
        $iso = Get-InstalledIso
        if ($iso -and (Test-Path -LiteralPath $iso -PathType Leaf)) { $len = (Get-Item -LiteralPath $iso).Length; foreach ($d in $Discs) { if ($d.Size -eq $len) { return $d.Serial } } }
    } catch { }
    return $null
}
function Get-SerialWarning($e, $disc) {
    if (-not $disc -or -not $e.ok -or @($e.serials).Count -eq 0) { return $null }
    if (@($e.serials | Where-Object { $_ -ieq $disc }).Count -gt 0) { return $null }
    return "Made for $(@($e.serials) -join ', '); your disc is $disc. Most world textures still match; menus, HUD and text may stay original."
}
function Test-ModEnabled([string]$id) { return $S.mods.active.Contains($id) }
function Test-ModLoaded([string]$id) { if (-not (Test-ModEnabled $id)) { return $false }; $mm = Get-ModMaster $S.mods $id; return (-not ($mm -and $S.mods.off[$mm])) }
function Get-ModGroup([string]$id) {
    # the switched-on mods in the same list on the page (script and native mods, or texture packs), in priority order
    $mm = Get-ModMaster $S.mods $id
    return , @($S.mods.active | Where-Object { (Get-ModMaster $S.mods $_) -eq $mm })
}
function Set-ModOn([string]$id, [bool]$on) {
    if (-not (Confirm-GameClosed)) { return }
    if ($on) {
        $e = Get-ModEntry $id
        $why = $(if ($e) { Get-ModBlock $e } else { 'mod not found' })
        if ($why) { Set-PageNote $PageView.mods "This mod cannot be switched on: $why." $cRed; return }
        if (-not $S.mods.active.Contains($id)) { $S.mods.active.Insert(0, $id) }      # at the top: highest priority
    } else { [void]$S.mods.active.Remove($id) }
    if (Save-ActiveFile) { Set-PageNote $PageView.mods $(if ($on) { 'Switched on. It is used the next time the game starts.' } else { 'Switched off.' }) $cGreen }
    Request-PageBuild 'mods'
}
function Move-Mod([string]$id, [int]$delta) {
    if (-not (Confirm-GameClosed)) { return }
    $g = Get-ModGroup $id
    $i = [array]::IndexOf($g, $id); $j = $i + $delta
    if ($i -lt 0 -or $j -lt 0 -or $j -ge $g.Count) { return }
    $a = $S.mods.active.IndexOf($id); $b = $S.mods.active.IndexOf([string]$g[$j])
    $S.mods.active[$a] = [string]$g[$j]; $S.mods.active[$b] = $id
    [void](Save-ActiveFile)
    Request-PageBuild 'mods'
}
function Set-ModMaster([string]$m, [bool]$on) {
    if (-not (Confirm-GameClosed)) { return }
    $S.mods.off[$m] = (-not $on)
    if (Save-ActiveFile) { Set-PageNote $PageView.mods 'Saved. It applies the next time the game starts.' $cGreen }
    Request-PageBuild 'mods'
}
function Remove-Mod([string]$id) {
    if (-not (Confirm-GameClosed)) { return }
    $e = Get-ModEntry $id
    if (-not $e) { return }
    $nm = Get-ModName $e
    if (-not $S.test) {
        $r = [System.Windows.Forms.MessageBox]::Show($form, "Remove `"$nm`"?`r`n`r`nIts folder goes to the Recycle Bin. Its settings stay in mods\settings.json, so they are back if you add it again.", $AppTitle, 'YesNo', 'Question')
        if ($r -ne [System.Windows.Forms.DialogResult]::Yes) { return }
    }
    try {
        if ($S.test) { Remove-Item -LiteralPath $e.dir -Recurse -Force }
        else {
            Add-Type -AssemblyName Microsoft.VisualBasic
            # all dialogs: Windows asks before it deletes for good a folder that cannot go to the Recycle Bin (a USB stick, a network drive)
            [Microsoft.VisualBasic.FileIO.FileSystem]::DeleteDirectory($e.dir, [Microsoft.VisualBasic.FileIO.UIOption]::AllDialogs, [Microsoft.VisualBasic.FileIO.RecycleOption]::SendToRecycleBin)
        }
    } catch [System.OperationCanceledException] { return }
    catch { Set-PageNote $PageView.mods ("The mod could not be removed: " + $_.Exception.Message) $cRed; return }
    [void]$S.mods.active.Remove($id)
    [void]$S.mods.entries.Remove($e)
    if (Save-ActiveFile) { Set-PageNote $PageView.mods "Removed `"$nm`"." $cGreen }
    if ($S.modOpenSet -eq $id) { $S.modOpenSet = '' }
    if ($S.modOpenInfo -eq $id) { $S.modOpenInfo = '' }
    Request-PageBuild 'mods'
}
function Get-ModFreeId([string]$base) {
    # a folder name for a new mod that is not taken: base, base-2, base-3, ...
    $taken = { param($x) return ((Get-ModEntry $x) -or (Test-Path -LiteralPath (Join-Path (Get-ModsDir) $x))) }
    if (-not (& $taken $base)) { return $base }
    for ($n = 2; $n -lt 1000; $n++) { if (-not (& $taken "$base-$n")) { return "$base-$n" } }
    return $base
}
function Get-ModSlug([string]$name) {
    $sx = (($name.ToLowerInvariant() -replace '[^a-z0-9]+', '-').Trim('-'))
    if ($sx.Length -gt 48) { $sx = $sx.Substring(0, 48).Trim('-') }
    if (-not $sx) { $sx = 'texture-pack' }
    return $sx
}

# ================================================================ adding a mod: a background runspace reads a .zip or a folder (what it is,
# which files), the page shows what it found, a second runspace copies it into mods\<id>.tmp and renames that to mods\<id> when complete
$ModInspectScript = {
    param($path, $sync)
    function Test-Hex([string]$sx, [int]$n) { return ($sx.Length -ge 1 -and $sx.Length -le $n -and $sx -match '^[0-9a-fA-F]+$') }
    function Test-Reg([string]$sx) { if (-not $sx.StartsWith('r')) { return $false }; $r = $sx.Substring(1); if ($r -match '^\d+x\d+$') { return $true }; return (Test-Hex $r 16) }
    function Test-Tex([string]$fname) {
        $m = [regex]::Match($fname, '^(.+)\.(png|dds)$', 'IgnoreCase')
        if (-not $m.Success) { return $false }
        $p = $m.Groups[1].Value.Split('-')
        if ($p.Count -eq 2) { return ((Test-Hex $p[0] 16) -and (Test-Hex $p[1] 8)) }
        if ($p.Count -eq 3) { return ((Test-Hex $p[0] 16) -and ((Test-Hex $p[1] 16) -or (Test-Reg $p[1])) -and (Test-Hex $p[2] 8)) }
        if ($p.Count -eq 4) { return ((Test-Hex $p[0] 16) -and (Test-Hex $p[1] 16) -and (Test-Reg $p[2]) -and (Test-Hex $p[3] 8)) }
        return $false
    }
    function Test-Serial([string]$sx) { return ($sx -match '^[A-Za-z]{4}-\d{5}$') }
    function Get-Leaf([string]$p) { $i = $p.LastIndexOf('/'); if ($i -ge 0) { return $p.Substring($i + 1) }; return $p }
    function Get-Under([string]$p, [string]$prefix) { if (-not $prefix) { return $p }; if ($p.StartsWith($prefix + '/', [StringComparison]::Ordinal)) { return $p.Substring($prefix.Length + 1) }; return $null }
    function Read-Entry($e) {
        if ($sync.isZip) {
            $z = [System.IO.Compression.ZipFile]::OpenRead($path)
            try { $sx = $z.GetEntry($e.raw).Open(); try { $rd = New-Object System.IO.StreamReader($sx, [System.Text.Encoding]::UTF8); return $rd.ReadToEnd() } finally { $sx.Dispose() } } finally { $z.Dispose() }
        }
        return [System.IO.File]::ReadAllText((Join-Path $path $e.raw), [System.Text.Encoding]::UTF8)
    }
    try {
        $entries = New-Object System.Collections.ArrayList
        $leaf = [System.IO.Path]::GetFileName($path.TrimEnd('\'))
        if (Test-Path -LiteralPath $path -PathType Container) {
            $base = (Get-Item -LiteralPath $path).FullName.TrimEnd('\')
            $sync.isZip = $false
            foreach ($f in [System.IO.Directory]::EnumerateFiles($base, '*', [System.IO.SearchOption]::AllDirectories)) {
                if ($sync.cancel) { $sync.state = 'cancel'; return }
                $rel = $f.Substring($base.Length + 1).Replace('\', '/')
                [void]$entries.Add(@{ path = $rel; raw = $rel.Replace('/', '\'); size = (New-Object System.IO.FileInfo($f)).Length })
                if ($entries.Count % 200 -eq 0) { $sync.n = $entries.Count }
            }
        } elseif (Test-Path -LiteralPath $path -PathType Leaf) {
            $ext = [System.IO.Path]::GetExtension($path).ToLowerInvariant()
            if ($ext -in @('.7z', '.rar', '.tar', '.gz', '.xz', '.bz2', '.zst')) { throw 'This window opens .zip files and folders only. Extract the archive first (for example with 7-Zip), then add the folder.' }
            if ($ext -ne '.zip') { throw 'Pick a .zip file or a folder.' }
            Add-Type -AssemblyName System.IO.Compression.FileSystem
            $sync.isZip = $true
            $z = $null
            try { $z = [System.IO.Compression.ZipFile]::OpenRead($path) } catch { throw "This is not a readable .zip file: $($_.Exception.Message)" }
            try {
                foreach ($en in $z.Entries) {
                    if ($sync.cancel) { $sync.state = 'cancel'; return }
                    $fname = $en.FullName
                    if ($fname.EndsWith('/') -or $fname.EndsWith('\')) { continue }
                    [void]$entries.Add(@{ path = $fname.Replace('\', '/').TrimStart('/'); raw = $fname; size = $en.Length })
                    if ($entries.Count % 200 -eq 0) { $sync.n = $entries.Count }
                }
            } finally { $z.Dispose() }
        } else { throw 'That file or folder could not be found.' }
        $sync.n = $entries.Count
        $hint = $leaf; if ($hint.ToLowerInvariant().EndsWith('.zip')) { $hint = $hint.Substring(0, $hint.Length - 4) }
        $sync.hint = $hint
        $layout = ''; $prefix = ''; $serial = $null; $loose = $false
        # 1. a script or native mod: omni-mod.json (or OpenRAC's openrac-mod.json) at the top or one folder down, format 2
        $mf = @($entries | Where-Object { (Get-Leaf $_.path) -in @('omni-mod.json', 'openrac-mod.json') -and ($_.path.Split('/').Count -le 2) } | Sort-Object { $_.path.Split('/').Count }, { if ((Get-Leaf $_.path) -ieq 'omni-mod.json') { 0 } else { 1 } })
        if ($mf.Count -gt 0) {
            $txt = Read-Entry $mf[0]
            $j = $null
            try { $j = $txt.TrimStart([char]0xFEFF) | ConvertFrom-Json } catch { throw "$(Get-Leaf $mf[0].path) could not be read: $(($_.Exception.Message -split "`r?`n")[0])" }
            if ($j -and $j.PSObject.Properties['format'] -and $j.format -eq 2) {
                $id = [string]$j.id
                if ($id -cnotmatch '^[a-z0-9]+(-[a-z0-9]+)*$' -or $id.Length -gt 64) { throw "$(Get-Leaf $mf[0].path) has an id that cannot be used ($id): lower case letters and digits joined by single hyphens." }
                $layout = 'script'; $prefix = $(if ($mf[0].path.Contains('/')) { $mf[0].path.Substring(0, $mf[0].path.LastIndexOf('/')) } else { '' })
                $sync.info = @{ id = $id; name = [string]$j.name; version = [string]$j.version; author = [string]$j.author }
            }
        }
        # 2. a mod with its own mod.json at the top or one folder down (the shallowest)
        if (-not $layout) {
            $mj = @($entries | Where-Object { (Get-Leaf $_.path) -ieq 'mod.json' -and ($_.path.Split('/').Count -le 2) } | Sort-Object { $_.path.Length })
            if ($mj.Count -gt 0) {
                $layout = 'modjson'; $prefix = $(if ($mj[0].path.Contains('/')) { $mj[0].path.Substring(0, $mj[0].path.LastIndexOf('/')) } else { '' })
                $sync.modjson = Read-Entry $mj[0]
            }
        }
        # 3. a PCSX2 texture pack: <SERIAL>/replacements/, else any folder called replacements, else the folder with the most texture files
        if (-not $layout) {
            $nameSerial = $(if (Test-Serial $hint) { $hint.ToUpperInvariant() } else { $null })
            $best = {
                # the prefix with the most files; on a tie the one that sorts first ('' = the top of the source is a prefix too)
                param($counts)
                $top = $null; $have = $false
                foreach ($k in @($counts.get_Keys())) { if (-not $have -or $counts[$k] -gt $counts[$top] -or ($counts[$k] -eq $counts[$top] -and [string]::CompareOrdinal($k, $top) -lt 0)) { $top = $k; $have = $true } }
                return $top
            }
            $bySerial = @{}; $byName = @{}; $serialOf = @{}
            foreach ($e in $entries) {
                $c = $e.path.Split('/')
                $dirs = $c.Count - 1
                for ($i = 0; $i -lt $dirs - 1; $i++) {
                    if ((Test-Serial $c[$i]) -and $c[$i + 1] -ieq 'replacements') { $k = ($c[0..($i + 1)] -join '/'); $bySerial[$k] = 1 + [int]$bySerial[$k]; $serialOf[$k] = $c[$i].ToUpperInvariant(); break }
                }
                for ($i = 0; $i -lt $dirs; $i++) {
                    if ($c[$i] -ieq 'replacements') {
                        $k = ($c[0..$i] -join '/'); $byName[$k] = 1 + [int]$byName[$k]
                        if (-not $serialOf.ContainsKey($k)) { $serialOf[$k] = $(if ($i -gt 0 -and (Test-Serial $c[$i - 1])) { $c[$i - 1].ToUpperInvariant() } else { $nameSerial }) }
                        break
                    }
                }
            }
            $pick = & $best $bySerial
            if ($null -eq $pick) { $pick = & $best $byName }
            if ($null -ne $pick) { $layout = 'pcsx2'; $prefix = $pick; $serial = $serialOf[$pick] }
            else {
                $byDir = @{}
                foreach ($e in $entries) { if (Test-Tex (Get-Leaf $e.path)) { $d = $(if ($e.path.Contains('/')) { $e.path.Substring(0, $e.path.LastIndexOf('/')) } else { '' }); $byDir[$d] = 1 + [int]$byDir[$d] } }
                $pick = & $best $byDir
                if ($null -ne $pick) {
                    $layout = 'pcsx2'; $prefix = $pick; $loose = $true
                    $serial = $nameSerial; if (-not $serial) { foreach ($c in $pick.Split('/')) { if (Test-Serial $c) { $serial = $c.ToUpperInvariant(); break } } }
                }
            }
        }
        if (-not $layout) { throw 'This does not look like a mod or a PCSX2 texture pack. A script mod has an omni-mod.json; a texture pack holds a folder called replacements (or files named like 0123456789abcdef-89abcdef.png).' }
        $files = New-Object System.Collections.ArrayList
        $bytes = [long]0
        foreach ($e in $entries) {
            $rel = Get-Under $e.path $prefix
            if ($null -eq $rel -or $rel -eq '') { continue }
            if ($loose -and -not (Test-Tex (Get-Leaf $rel))) { continue }
            [void]$files.Add(@{ raw = $e.raw; rel = $rel; size = $e.size }); $bytes += $e.size
        }
        if ($files.Count -eq 0) { throw 'No files were found in it.' }
        $sync.layout = $layout; $sync.prefix = $prefix; $sync.serial = $serial; $sync.files = $files; $sync.bytes = $bytes
        $sync.state = 'ok'
    } catch {
        $sync.msg = $_.Exception.Message
        $sync.state = 'bad'
    }
}
$ModImportScript = {
    param($job, $sync)
    function Get-SafeRel([string]$rel) {
        # a path inside the mod folder: no .., no drive, no characters Windows refuses, reserved names get a _
        $parts = New-Object System.Collections.Generic.List[string]
        foreach ($c in $rel.Split(@('/', '\'), [StringSplitOptions]::None)) {
            if ($c -eq '.' -or $c -eq '..' -or $c.Contains(':')) { return $null }
            $c = $c.TrimEnd(' ', '.')
            if (-not $c) { return $null }
            $c = [regex]::Replace($c, '[\x00-\x1f<>"|?*]', '_')
            $stem = $c.Split('.')[0].TrimEnd(' ').ToUpperInvariant()
            if ($stem -in @('CON', 'PRN', 'AUX', 'NUL', 'CONIN$', 'CONOUT$') -or $stem -match '^(COM|LPT)[1-9]$') { $c = '_' + $c }
            $parts.Add($c)
        }
        if ($parts.Count -eq 0) { return $null }
        return ($parts -join '\')
    }
    $tmp = Join-Path $job.modsDir ($job.id + '.tmp')
    $fin = Join-Path $job.modsDir $job.id
    $zip = $null
    try {
        if (Test-Path -LiteralPath $fin) { throw 'A mod with that id is already installed.' }
        if (Test-Path -LiteralPath $tmp) { Remove-Item -LiteralPath $tmp -Recurse -Force }
        [void][System.IO.Directory]::CreateDirectory($tmp)
        if ($job.isZip) { Add-Type -AssemblyName System.IO.Compression.FileSystem; $zip = [System.IO.Compression.ZipFile]::OpenRead($job.path) }
        $taken = @{}
        $made = @{}
        $buf = New-Object byte[] (1MB)
        foreach ($f in $job.files) {
            if ($sync.cancel) { throw 'Cancelled.' }
            if ($job.layout -eq 'modjson' -and $f.rel -ieq 'mod.json') { $sync.filesDone++; continue }      # written below, with its id
            $rp = Get-SafeRel $f.rel
            if (-not $rp -or $taken.ContainsKey($rp)) { $sync.skipped++; $sync.filesDone++; $sync.bytesDone += $f.size; continue }
            $taken[$rp] = $true
            $dest = $(if ($job.layout -eq 'pcsx2') { Join-Path (Join-Path $tmp 'textures') $rp } else { Join-Path $tmp $rp })
            $dd = [System.IO.Path]::GetDirectoryName($dest)
            if (-not $made.ContainsKey($dd)) { [void][System.IO.Directory]::CreateDirectory($dd); $made[$dd] = $true }
            $sync.current = $f.rel
            $src = $(if ($zip) { $zip.GetEntry($f.raw).Open() } else { [System.IO.File]::OpenRead((Join-Path $job.path $f.raw)) })
            try {
                $out = [System.IO.File]::Create($dest)
                try {
                    while (($n = $src.Read($buf, 0, $buf.Length)) -gt 0) {
                        if ($sync.cancel) { throw 'Cancelled.' }
                        $out.Write($buf, 0, $n); $sync.bytesDone += $n
                    }
                } finally { $out.Dispose() }
            } finally { $src.Dispose() }
            $sync.filesDone++
        }
        if ($job.modjson) { [System.IO.File]::WriteAllText((Join-Path $tmp 'mod.json'), $job.modjson, (New-Object System.Text.UTF8Encoding($false))) }
        if ($zip) { $zip.Dispose(); $zip = $null }
        [System.IO.Directory]::Move($tmp, $fin)
        $sync.state = 'ok'
    } catch {
        $sync.msg = $_.Exception.Message
        $sync.state = $(if ($sync.cancel) { 'cancel' } else { 'bad' })
        if ($zip) { try { $zip.Dispose() } catch { }; $zip = $null }
        try { if (Test-Path -LiteralPath $tmp) { Remove-Item -LiteralPath $tmp -Recurse -Force } } catch { }
    }
}
function Start-ModInspect([string]$path) {
    if (-not (Confirm-GameClosed)) { return }
    if ($S.modJob -and $S.modJob.phase -in @('reading', 'importing')) { Set-PageNote $PageView.mods 'Please wait until the mod that is being read or added is finished.' $cAccent; return }
    $sync = [hashtable]::Synchronized(@{ state = 'run'; msg = ''; n = 0; cancel = $false; isZip = $false; layout = ''; prefix = ''; serial = $null; files = $null; bytes = [long]0; hint = ''; info = $null; modjson = $null })
    $ps = [powershell]::Create()
    $null = $ps.AddScript($ModInspectScript).AddArgument($path).AddArgument($sync)
    $S.modJob = @{ phase = 'reading'; ps = $ps; sync = $sync; handle = $ps.BeginInvoke(); path = $path; name = [System.IO.Path]::GetFileName($path.TrimEnd('\')) }
    Request-PageBuild 'mods'
}
function Stop-ModJob {
    $j = $S.modJob
    if ($j -and $j.sync) { $j.sync.cancel = $true; try { $null = $j.ps.BeginStop($null, $null) } catch { } }
    $S.modJob = $null
}
function Get-SizeText([long]$b) {
    if ($b -ge 1GB) { return ('{0:N1} GB' -f ($b / 1GB)) }
    if ($b -ge 1MB) { return ('{0:N0} MB' -f ($b / 1MB)) }
    return ('{0:N0} KB' -f [math]::Ceiling($b / 1KB))
}
function Get-ModImportPlan($j) {
    # the id and the mod.json text a found mod gets; @{ err } when it cannot be added
    $sx = $j.sync
    if ($sx.layout -eq 'script') {
        $have = Get-ModEntry $sx.info.id
        if ($have) { return @{ err = "A mod with the id `"$($sx.info.id)`" is already installed (folder $($have.folder)). Remove it first to add this copy." } }
        if (Test-Path -LiteralPath (Join-Path (Get-ModsDir) $sx.info.id)) { return @{ err = "The folder mods\$($sx.info.id) is already there. Move it away first to add this mod." } }
        return @{ id = $sx.info.id; name = $(if ($sx.info.name) { $sx.info.name } else { $sx.info.id }); modjson = $null }
    }
    if ($sx.layout -eq 'modjson') {
        $r = Read-JsonText $sx.modjson
        if (-not $r.ok -or -not (Test-JDict $r.value)) { return @{ err = "mod.json could not be read: $(if ($r.ok) { 'it is not a JSON object' } else { $r.err })" } }
        $o = $r.value
        $id = Get-JVal $o 'id'
        if (-not ($id -is [string] -and $id -cmatch '^[a-z0-9][a-z0-9._-]{0,63}$')) { return @{ err = "mod.json has an id that cannot be used as a folder name ($id); it must be lower-case letters, digits, . _ -" } }
        $id = Get-ModFreeId $id
        $o['id'] = $id
        $nm = Get-JVal $o 'name'
        return @{ id = $id; name = $(if ($nm -is [string] -and $nm) { $nm } else { $id }); modjson = ((ConvertTo-JsonText $o '') + "`n") }
    }
    $name = $(if ($j.nameBox -and $j.nameBox.Text.Trim()) { $j.nameBox.Text.Trim() } else { $sx.hint })
    if (-not $name) { $name = 'Texture pack' }
    $id = Get-ModFreeId (Get-ModSlug $name)
    $info = [ordered]@{ format = 1; id = $id; name = $name; author = ''; version = '1.0'; description = "PCSX2 texture pack, $($sx.files.Count) files."; game = 'rac1'; kind = 'textures'; serials = (New-Object System.Collections.ArrayList); source = 'pcsx2' }
    if ($sx.serial) { [void]$info.serials.Add($sx.serial) }
    return @{ id = $id; name = $name; modjson = ((ConvertTo-JsonText $info '') + "`n") }
}
function Start-ModImport {
    $j = $S.modJob
    if (-not $j -or $j.phase -ne 'preview') { return }
    if (-not (Confirm-GameClosed)) { return }
    $plan = Get-ModImportPlan $j
    if ($plan.err) { $S.modJob = @{ phase = 'error'; msg = $plan.err }; Request-PageBuild 'mods'; return }
    $md = Get-ModsDir
    if (-not (Test-Path -LiteralPath $md)) { [void][System.IO.Directory]::CreateDirectory($md) }
    $sx = $j.sync
    $job = @{ path = $j.path; isZip = $sx.isZip; layout = $sx.layout; files = $sx.files; modsDir = $md; id = $plan.id; modjson = $plan.modjson }
    $sync = [hashtable]::Synchronized(@{ state = 'run'; msg = ''; cancel = $false; filesDone = 0; bytesDone = [long]0; skipped = 0; current = '' })
    $ps = [powershell]::Create()
    $null = $ps.AddScript($ModImportScript).AddArgument($job).AddArgument($sync)
    $S.modJob = @{ phase = 'importing'; ps = $ps; sync = $sync; handle = $ps.BeginInvoke(); name = $plan.name; id = $plan.id; total = $sx.files.Count; bytes = $sx.bytes }
    Request-PageBuild 'mods'
}
function Tick-ModsJob {
    $j = $S.modJob
    if (-not $j -or $j.phase -notin @('reading', 'importing')) { return }
    $sx = $j.sync
    if ($sx.state -eq 'run') {
        if ($S.modJobLbl -and -not $S.modJobLbl.IsDisposed) {
            if ($j.phase -eq 'reading') { $S.modJobLbl.Text = $(if ($sx.n -gt 0) { "$($j.name)  -  $('{0:N0}' -f $sx.n) files found so far" } else { "$($j.name)  -  opening it" }) }
            else {
                $pct = $(if ($j.bytes -gt 0) { [int][math]::Min(100, [math]::Floor(100.0 * $sx.bytesDone / $j.bytes)) } else { 0 })
                $S.modJobLbl.Text = ('{0}%  -  {1:N0} of {2:N0} files  -  {3} of {4}' -f $pct, $sx.filesDone, $j.total, (Get-SizeText $sx.bytesDone), (Get-SizeText $j.bytes))
                if ($S.modJobBar -and -not $S.modJobBar.IsDisposed) { Set-Bar $S.modJobBar $pct }
            }
        }
        return
    }
    if (-not $j.handle.IsCompleted) { return }
    try { $j.ps.Dispose() } catch { }
    if ($j.phase -eq 'reading') {
        if ($sx.state -eq 'ok') { $S.modJob = @{ phase = 'preview'; sync = $sx; path = $j.path; name = $j.name; nameBox = $null } }
        elseif ($sx.state -eq 'cancel') { $S.modJob = $null }
        else { $S.modJob = @{ phase = 'error'; msg = $sx.msg } }
        Request-PageBuild 'mods'
        return
    }
    # an import finished: switch the new mod on (at the top), as the RAC-RS Launcher does
    $S.modJob = $null
    if ($sx.state -eq 'ok') {
        $S.mods = Read-ModStore
        $e = Get-ModEntry $j.id
        $why = $(if ($e) { Get-ModBlock $e } else { 'it was not found after copying' })
        $skip = $(if ($sx.skipped -gt 0) { " ($($sx.skipped) files left out: unusable or repeated names)" } else { '' })
        if ($why) { Set-PageNote $PageView.mods "Added `"$($j.name)`"$skip, but it cannot be switched on: $why." $cAccent }
        else {
            if (-not $S.mods.active.Contains($j.id)) { $S.mods.active.Insert(0, $j.id) }
            if (Save-ActiveFile) { Set-PageNote $PageView.mods "Added `"$($j.name)`" and switched it on$skip. It is used the next time the game starts." $cGreen }
        }
    } elseif ($sx.state -eq 'bad') { $S.modJob = @{ phase = 'error'; msg = $sx.msg } }
    Request-PageBuild 'mods'
}
function Get-Pcsx2Candidates {
    $docs = [Environment]::GetFolderPath('MyDocuments')
    $serials = @()
    $d = Get-DiscSerial
    if ($d) { $serials = @($d) } else { $serials = @($Discs | ForEach-Object { $_.Serial }) }
    $base = Join-Path $docs 'PCSX2\textures'
    $out = @()
    foreach ($sx in $serials) {
        $p = Join-Path $base $sx
        if (Test-Path -LiteralPath $p -PathType Container) { $out += , @($sx, $p) }
        $z = Join-Path $base "$sx.zip"
        if (Test-Path -LiteralPath $z -PathType Leaf) { $out += , @($sx, $z) }
    }
    return @{ list = $out; where = (Join-Path $base $serials[0]) }
}

# ================================================================ building the Mods page
function Add-ModsToolbar($c, [int]$y) {
    $busy = ($S.modJob -and $S.modJob.phase -in @('reading', 'importing'))
    $x = 0
    $btns = @(@('Add .zip', 'zip'), @('Add folder', 'dir'), @('Import from PCSX2', 'pcsx2'), @('Open mods folder', 'open'))
    foreach ($b in $btns) {
        $w = (Measure-TextW $b[0] $fN) + (Px 26)
        if ($x + $w -gt $c.Width) { $x = 0; $y += Px 34 }
        $btn = Add-PageButton $c $b[0] $x $y $b[1] {
            param($sx, $e)
            try {
                switch ($sx.Tag) {
                    'zip' {
                        $dlg = New-Object System.Windows.Forms.OpenFileDialog
                        $dlg.Title = 'Pick a mod or a texture pack (.zip)'; $dlg.Filter = 'Zip files (*.zip)|*.zip|All files (*.*)|*.*'
                        if ($dlg.ShowDialog($form) -eq [System.Windows.Forms.DialogResult]::OK) { Start-ModInspect $dlg.FileName }
                    }
                    'dir' {
                        $dlg = New-Object System.Windows.Forms.FolderBrowserDialog
                        $dlg.Description = 'Pick the folder of a mod or a texture pack'; $dlg.ShowNewFolderButton = $false
                        if ($dlg.ShowDialog($form) -eq [System.Windows.Forms.DialogResult]::OK) { Start-ModInspect $dlg.SelectedPath }
                    }
                    'pcsx2' {
                        $r = Get-Pcsx2Candidates
                        if ($r.list.Count -eq 0) { $S.modJob = @{ phase = 'error'; msg = "No PCSX2 texture folder was found for your disc. PCSX2 keeps them in:`r`n$($r.where)\replacements" } }
                        else { $S.modJob = @{ phase = 'pcsx2'; list = $r.list } }
                        Request-PageBuild 'mods'
                    }
                    'open' { $d = Get-ModsDir; if (-not (Test-Path -LiteralPath $d)) { [void][System.IO.Directory]::CreateDirectory($d) }; Open-Path $d }
                }
            } catch { Show-PageError $_ }
        }
        if ($busy -and $b[1] -ne 'open') { $btn.Enabled = $false }
        $x = $btn.Right + (Px 8)
    }
    return ($y + (Px 38))
}
function Add-ModsJobCard($c, [int]$y) {
    $j = $S.modJob
    $S.modJobLbl = $null; $S.modJobBar = $null
    if (-not $j) { return $y }
    $pad = Px 14
    $card = New-Card $c $y $(if ($j.phase -eq 'error') { $cRed } else { $null })
    $inW = $card.Width - 2 * $pad
    $cy = Px 12
    $buttons = @()
    switch ($j.phase) {
        'error' {
            $l = Add-Text $card $j.msg $pad $cy $inW $fN $cText; $cy = $l.Bottom + (Px 8)
            $buttons = @(@('OK', 'close'))
        }
        'pcsx2' {
            $l = Add-Text $card 'PCSX2 texture folders found' $pad $cy $inW $fRow $cText; $cy = $l.Bottom + (Px 4)
            foreach ($it in $j.list) {
                $b = Add-PageButton $card 'Look at it' 0 $cy $it[1] { param($sx, $e) Start-ModInspect ([string]$sx.Tag) }
                $b.Left = $pad + $inW - $b.Width
                $t = Add-Text $card "$($it[0])  -  $($it[1])" $pad ($cy + (Px 5)) ($inW - $b.Width - (Px 10)) $fSmall $cGrey
                $cy = [math]::Max($b.Bottom, $t.Bottom) + (Px 6)
            }
            $buttons = @(@('Cancel', 'close'))
        }
        'reading' {
            $l = Add-Text $card 'Reading the mod...' $pad $cy $inW $fRow $cText; $cy = $l.Bottom + (Px 4)
            $S.modJobLbl = Add-Text $card "$($j.name)  -  opening it" $pad $cy $inW $fSmall $cGrey; $cy = $S.modJobLbl.Bottom
            $l = Add-Text $card 'A big pack can take a moment. The window stays usable.' $pad $cy $inW $fSmall $cGrey; $cy = $l.Bottom + (Px 8)
            $buttons = @(@('Cancel', 'cancel'))
        }
        'preview' {
            $sx = $j.sync
            $what = $(if ($sx.layout -eq 'script') { 'A script mod' } elseif ($sx.layout -eq 'modjson') { 'A mod with its own description' } else { 'A PCSX2 texture pack' })
            $made = $(if ($sx.serial) { "  -  made for $($sx.serial)" } else { '' })
            $l = Add-Text $card 'Add this mod?' $pad $cy $inW $fRow $cText; $cy = $l.Bottom + (Px 2)
            $title = $(if ($sx.layout -eq 'script' -and $sx.info) { "$($sx.info.name) $(if ($sx.info.version) { 'v' + $sx.info.version })  -  " } else { '' })
            $l = Add-Text $card ("$title$what  -  $('{0:N0}' -f $sx.files.Count) files, $(Get-SizeText $sx.bytes)$made") $pad $cy $inW $fSmall $cGrey; $cy = $l.Bottom + (Px 4)
            $disc = Get-DiscSerial
            if ($sx.serial -and $disc -and $sx.serial -ine $disc) { $l = Add-Text $card "Your disc is $disc. Most world textures still match; menus, HUD and text may stay original." $pad $cy $inW $fSmall $cAccent; $cy = $l.Bottom + (Px 4) }
            if ($sx.layout -eq 'pcsx2') {
                $l = Add-Text $card 'Name' $pad ($cy + (Px 4)) (Px 60) $fN $cText
                $tb = New-Object System.Windows.Forms.TextBox
                $tb.Location = New-Object System.Drawing.Point(($pad + (Px 64)), $cy); $tb.Size = New-Object System.Drawing.Size([math]::Min((Px 300), $inW - (Px 64)), (Px 26))
                $tb.Font = $fN; $tb.BackColor = $cField; $tb.ForeColor = $cText; $tb.MaxLength = 80
                $tb.Text = $(if ($j.nameBox) { $j.nameBox.Text } else { $sx.hint })
                $card.Controls.Add($tb); $j.nameBox = $tb
                $cy = $tb.Bottom + (Px 8)
            }
            $buttons = @(@('Add mod', 'go'), @('Cancel', 'close'))
        }
        'importing' {
            $l = Add-Text $card "Adding `"$($j.name)`"..." $pad $cy $inW $fRow $cText; $cy = $l.Bottom + (Px 6)
            $S.modJobBar = New-Bar $pad $cy $inW (Px 8); $card.Controls.Add($S.modJobBar); $cy = $S.modJobBar.Bottom + (Px 6)
            $S.modJobLbl = Add-Text $card '0%' $pad $cy $inW $fSmall $cGrey; $cy = $S.modJobLbl.Bottom + (Px 8)
            $buttons = @(@('Cancel', 'cancel'))
        }
    }
    $x = $pad
    foreach ($b in $buttons) {
        $btn = Add-PageButton $card $b[0] $x $cy $b[1] {
            param($sx, $e)
            switch ($sx.Tag) {
                'close' { $S.modJob = $null }
                'cancel' { Stop-ModJob; Set-PageNote $PageView.mods 'Cancelled.' $cGrey }
                'go' { Start-ModImport; return }
            }
            Request-PageBuild 'mods'
        } (Px 80)
        if ($b[1] -eq 'go') { $btn.BackColor = $cAccent; $btn.ForeColor = [System.Drawing.Color]::White; $btn.UseVisualStyleBackColor = $false; $btn.Font = $fBtn }
        $x = $btn.Right + (Px 8)
        $cy2 = $btn.Bottom
    }
    $card.Height = $cy2 + (Px 12)
    return ($card.Bottom + (Px 10))
}
function Get-MasterText([string]$m, [bool]$on, [int]$n, [int]$k) {
    $noun = $(if ($m -eq 'textures') { 'packs' } else { 'mods' })
    if ($n -eq 0) { if ($m -eq 'textures') { return 'No texture packs yet. Add one above.' }; return 'No script mods yet. Add one above, or put its folder into the mods folder.' }
    if (-not $on) { return "Off: the game loads no $noun. Your switches below are kept for when you switch this on." }
    return "$k of $n $noun $(if ($k -eq 1) { 'is' } else { 'are' }) switched on below."
}
function Add-ModsMasterCard($c, [int]$y) {
    $st = $S.mods
    if ($st.entries.Count -eq 0 -and -not $st.off.textures -and -not $st.off.mods) { return $y }
    $pad = Px 14
    $card = New-Card $c $y
    $inW = $card.Width - 2 * $pad
    $cy = Px 12
    $first = $true
    foreach ($m in @('textures', 'mods')) {
        $ents = @($st.entries | Where-Object { (Get-ModMaster $st $_.id) -eq $m })
        $n = $ents.Count; $k = @($ents | Where-Object { Test-ModEnabled $_.id }).Count
        $on = -not $st.off[$m]
        if (-not $first) { Add-Line $card $pad ($cy + (Px 5)) $inW; $cy += Px 11 }
        $first = $false
        $sw = New-Switch $on { param($ctx, $v) Set-ModMaster $ctx $v } $m (-not ($n -eq 0 -and $on))
        $sw.Location = New-Object System.Drawing.Point($pad, ($cy + (Px 2)))
        $card.Controls.Add($sw)
        $tx = $pad + $sw.Width + (Px 12)
        $l1 = Add-Text $card $(if ($m -eq 'textures') { 'Load texture packs' } else { 'Load mods' }) $tx $cy ($inW - $tx + $pad) $fRow $cText
        $l2 = Add-Text $card (Get-MasterText $m $on $n $k) $tx $l1.Bottom ($inW - $tx + $pad) $fSmall $cGrey
        $cy = [math]::Max($l2.Bottom, $sw.Bottom)
    }
    $dll = Join-Path $root 'build\release\omniloader.dll'
    if (-not $S.test -and (Test-Path -LiteralPath (Join-Path $root 'build\release\rac-rs.exe')) -and -not (Test-Path -LiteralPath $dll)) {
        $l = Add-Text $card 'The mod loader (omniloader.dll) is not next to the game, so script mods cannot run. Repair / rebuild on the home page puts it back.' $pad ($cy + (Px 8)) $inW $fSmall $cAccent
        $cy = $l.Bottom
    }
    $card.Height = $cy + (Px 12)
    return ($card.Bottom + (Px 10))
}
function Add-ModsEmpty($c, [int]$y) {
    $pad = Px 14
    $card = New-Card $c $y
    $inW = $card.Width - 2 * $pad
    $l = Add-Text $card 'No mods yet' $pad (Px 12) $inW $fRowBig $cText
    $cy = $l.Bottom + (Px 4)
    foreach ($t in @('Click "Add .zip" or "Add folder" to add a script mod or a texture pack, or "Import from PCSX2" if you already use texture packs there.',
            'A texture pack is a folder called "replacements" with picture files in it, as PCSX2 uses.',
            'A script mod is a folder with an omni-mod.json in it. Arsenal Mastery comes with the game; Repair / rebuild puts it back when it is missing.')) {
        $l = Add-Text $card $t $pad $cy $inW $fSmall $cGrey; $cy = $l.Bottom + (Px 4)
    }
    $card.Height = $cy + (Px 8)
    return ($card.Bottom + (Px 10))
}
function Get-ShortText([string]$t, [int]$max) {
    if ($t.Length -le $max) { return $t }
    $cut = $t.Substring(0, $max)
    $i = $cut.LastIndexOf(' ')
    if ($i -gt 0) { $cut = $cut.Substring(0, $i) }
    return ($cut.TrimEnd(',', '.', ';') + '...')
}
function Get-ModSubtitle($e) {
    if (-not $e.ok) { return "Folder: mods\$($e.folder)" }      # (the problem itself is the orange line under it)
    $parts = @()
    if ($e.author) { $parts += "by $($e.author)" }
    if ($e.version) { $parts += "v$($e.version)" }
    if ($e.desc) { $parts += (Get-ShortText (($e.desc -split "`r?`n")[0]) 110) }
    return ($parts -join '  -  ')
}
function Get-KindText($e) {
    if ($e.kind -eq 'textures') { return 'Texture pack' }
    if ($e.kind -eq 'script') { return 'Script mod' }
    if ($e.kind -eq 'native') { return 'Native mod' }
    return [string]$e.kind
}
function Add-ModRow($c, [int]$y, $e, [string]$disc) {
    $id = $e.id
    $on = Test-ModEnabled $id
    $held = ($on -and -not (Test-ModLoaded $id))
    $block = Get-ModBlock $e
    $pad = Px 14
    $card = New-Card $c $y $(if ($on -and -not $held) { $cAccent } else { $null })
    $inW = $card.Width - 2 * $pad
    $sw = New-Switch $on { param($ctx, $v) Set-ModOn $ctx $v } $id ((-not $block) -or $on)
    $sw.Tag.muted = $held
    $sw.Location = New-Object System.Drawing.Point($pad, (Px 14))
    $card.Controls.Add($sw)
    $tx = $pad + $sw.Width + (Px 12); $tw = $inW - $tx + $pad
    $tc = $(if ($held) { $cDim } else { $cText })
    $nm = Add-Text $card (Get-ModName $e) $tx (Px 12) ([math]::Min($tw - (Px 96), (Measure-TextW (Get-ModName $e) $fRowBig) + (Px 4))) $fRowBig $tc
    $chip = New-Label (Get-KindText $e) ($nm.Right + (Px 8)) ($nm.Top + (Px 3)) ((Measure-TextW (Get-KindText $e) $fChip) + (Px 12)) (Px 18) $fChip $cBlue
    $chip.BackColor = $cChipBg; $chip.TextAlign = 'MiddleCenter'
    $card.Controls.Add($chip)
    $cy = [math]::Max($nm.Bottom, $chip.Bottom)
    $l = Add-Text $card (Get-ModSubtitle $e) $tx $cy $tw $fSmall $(if ($held) { $cDim } else { $cGrey }); $cy = $l.Bottom
    $warn = $block
    if (-not $warn) { $warn = Get-SerialWarning $e $disc }
    if ($held) { $warn = $(if ($e.kind -eq 'textures') { 'Load texture packs is off above, so the game does not load it.' } else { 'Load mods is off above, so the game does not load it.' }) }
    if ($warn) { $l = Add-Text $card $warn $tx $cy $tw $fSmall $cAccent; $cy = $l.Bottom }
    # the buttons: higher / lower priority (switched on), Mod settings, Details, Remove
    $cy += Px 6
    $x = $tx
    $grp = Get-ModGroup $id
    $pos = [array]::IndexOf($grp, $id)
    $list = @()
    if ($on) { $list += , @([string][char]0x25B2, 'up', ($pos -gt 0)); $list += , @([string][char]0x25BC, 'down', ($pos -ge 0 -and $pos + 1 -lt $grp.Count)) }
    $hasSet = ($e.sm -and $e.sm.settings.Count -gt 0)
    if ($hasSet) { $list += , @($(if ($S.modOpenSet -eq $id) { 'Hide settings' } else { 'Mod settings' }), 'set', $true) }
    $list += , @($(if ($S.modOpenInfo -eq $id) { 'Hide details' } else { 'Details' }), 'info', $true)
    $list += , @('Remove', 'remove', $true)
    foreach ($b in $list) {
        $w = $(if ($b[1] -in @('up', 'down')) { Px 32 } else { 0 })
        if ($x + [math]::Max($w, (Measure-TextW $b[0] $fN) + (Px 26)) -gt $pad + $inW) { $x = $tx; $cy += Px 34 }
        $btn = Add-PageButton $card $b[0] $x $cy @{ id = $id; act = $b[1] } {
            param($sx, $e2)
            try {
                $t = $sx.Tag
                switch ($t.act) {
                    'up' { Move-Mod $t.id -1 }
                    'down' { Move-Mod $t.id 1 }
                    'set' { $S.modOpenSet = $(if ($S.modOpenSet -eq $t.id) { '' } else { $t.id }); $S.modListen = $null; Request-PageBuild 'mods' }
                    'info' { $S.modOpenInfo = $(if ($S.modOpenInfo -eq $t.id) { '' } else { $t.id }); Request-PageBuild 'mods' }
                    'remove' { Remove-Mod $t.id }
                }
            } catch { Show-PageError $_ }
        } $w
        if (-not $b[2]) { $btn.Enabled = $false }
        if ($b[1] -eq 'up') { $btn.AccessibleName = $(if ($e.kind -eq 'textures') { 'Higher priority: wins when two packs change the same picture' } else { 'Earlier in the list: the game loads it first' }) }
        if ($b[1] -eq 'down') { $btn.AccessibleName = 'Lower priority' }
        $x = $btn.Right + (Px 6)
    }
    $cy += Px 30
    if ($hasSet -and $S.modOpenSet -eq $id) {
        Add-Line $card $pad ($cy + (Px 6)) $inW; $cy += Px 14
        $cy = Add-ModSettingsPanel $card $cy $e
    }
    if ($S.modOpenInfo -eq $id) {
        Add-Line $card $pad ($cy + (Px 6)) $inW; $cy += Px 14
        if ($e.desc) { $l = Add-Text $card $e.desc $pad $cy $inW $fSmall $cText; $cy = $l.Bottom + (Px 4) }
        if ($e.sm) {
            $games = @($e.sm.games.get_Keys() | ForEach-Object { "$_ ($(@($e.sm.games[$_]) -join ', '))" }) -join '; '
            $l = Add-Text $card "Made for: $games    Id: $($e.id)    Folder name: $($e.folder)" $pad $cy $inW $fSmall $cGrey; $cy = $l.Bottom
        } else {
            $ser = $(if (@($e.serials).Count -gt 0) { @($e.serials) -join ', ' } else { 'any disc' })
            $l = Add-Text $card "Made for: $ser    Source: $(if ($e.source) { $e.source } else { 'unknown' })    Folder name: $($e.folder)" $pad $cy $inW $fSmall $cGrey; $cy = $l.Bottom
        }
        $l = Add-Text $card "Location: $($e.dir)" $pad $cy $inW $fSmall $cGrey; $cy = $l.Bottom + (Px 6)
        $b = Add-PageButton $card 'Open its folder' $pad $cy $e.dir { param($sx, $e2) Open-Path ([string]$sx.Tag) }
        $cy = $b.Bottom + (Px 4)
    }
    $card.Height = $cy + (Px 12)
    return ($card.Bottom + (Px 8))
}

# --- a script mod's settings panel: preset buttons, one row per setting (a switch, choices or a list, a slider or number box, a text box,
# "press a key" buttons), the advanced ones folded away, Reset to defaults. Every change is written to mods\settings.json at once.
function Set-ModSetting([string]$id, $changes) {
    if (-not (Confirm-GameClosed)) { Request-PageBuild 'mods'; return }
    $e = Get-ModEntry $id
    try { Set-ModSettingValues $id $changes; Set-PageNote $PageView.mods 'Saved. The game uses it the next time it starts.' $cGreen }
    catch { Set-PageNote $PageView.mods ("The settings of `"$(if ($e) { Get-ModName $e } else { $id })`" could not be saved: " + $_.Exception.Message) $cRed }
    $S.modListen = $null
    Request-PageBuild 'mods'
}
function Format-ModNumber($d, $v) {
    if ($d.type -eq 'int') { return ([string][long]$v) }
    return ([double]$v).ToString('0.####', [System.Globalization.CultureInfo]::InvariantCulture)
}
function New-ChoiceMenu([string[]]$labels, [int]$sel, [scriptblock]$onPick, $ctx, [int]$maxW) {
    # a theme button that shows the chosen value; a click opens the choices as a menu (the chosen one in orange)
    $arrow = '  ' + [char]0x25BE
    $tw = 0
    foreach ($l in $labels) { $tw = [math]::Max($tw, (Measure-TextW ($l + $arrow) $fN)) }
    $b = New-Button ($(if ($sel -ge 0) { $labels[$sel] } else { 'Choose...' }) + $arrow) 0 0 ([math]::Min($maxW, $tw + (Px 30))) (Px 28) $false
    $b.Tag = @{ labels = $labels; sel = $sel; onPick = $onPick; ctx = $ctx }
    $b.Add_Click({
        param($snd, $ev)
        $t = $snd.Tag
        $m = New-Object System.Windows.Forms.ContextMenuStrip
        $m.ShowImageMargin = $false; $m.ShowCheckMargin = $false; $m.RenderMode = 'System'
        $m.BackColor = $cField; $m.ForeColor = $cText; $m.Font = $fN; $m.Tag = $t
        for ($i = 0; $i -lt $t.labels.Count; $i++) {
            $it = New-Object System.Windows.Forms.ToolStripMenuItem([string]$t.labels[$i])
            $it.Tag = $i; $it.BackColor = $cField; $it.ForeColor = $(if ($i -eq $t.sel) { $cAccent } else { $cText })
            $it.Add_Click({ param($s2, $e2) $o = $s2.Owner.Tag; $i2 = [int]$s2.Tag; if ($i2 -ne $o.sel) { try { & $o.onPick $o.ctx $i2 } catch { Show-PageError $_ } } })
            [void]$m.Items.Add($it)
        }
        $m.Show($snd, (New-Object System.Drawing.Point(0, $snd.Height)))
    })
    return $b
}
function New-ModSettingControl($e, $d, $cur, [int]$room, [int]$full) {
    $id = $e.id
    $ctx = @{ id = $id; d = $d }
    $v = $cur[$d.key]
    switch ($d.type) {
        'bool' { return (New-Switch ([bool]$v) { param($c, $on) Set-ModSetting $c.id @(@{ key = $c.d.key; value = [bool]$on }) } $ctx) }
        'choice' {
            $labels = [string[]]@($d.choices | ForEach-Object { $_.name })
            $sel = -1; for ($i = 0; $i -lt $d.choices.Count; $i++) { if ($d.choices[$i].value -ceq $v) { $sel = $i } }
            # the choices as one strip while it fits the card (beside the name, or under it); a list too wide for that is a button with a menu
            $pick = { param($c, $i) Set-ModSetting $c.id @(@{ key = $c.d.key; value = $c.d.choices[$i].value }) }
            $seg = New-Segmented $labels $sel $pick $ctx
            if ($seg.Width -le $full) { return $seg }
            $seg.Dispose()
            return (New-ChoiceMenu $labels $sel $pick $ctx $full)
        }
        { $_ -in @('int', 'float') } {
            $p = New-Object System.Windows.Forms.Panel
            $p.BackColor = $cCard
            if ($null -ne $d.min -and $null -ne $d.max) {
                $step = $(if ($d.type -eq 'int') { 1 } elseif ($d.step) { $d.step } else { 0 })
                $p.Size = New-Object System.Drawing.Size((Px 230), (Px 26))
                $vl = New-Label (Format-ModNumber $d $v) (Px 178) 0 (Px 52) (Px 26) $fN $cText
                $vl.TextAlign = 'MiddleRight'; $vl.BackColor = $cCard
                $ctx.vlbl = $vl
                $sl = New-Slider $d.min $d.max $step ([double]$v) (Px 172) { param($c, $x) $c.vlbl.Text = (Format-ModNumber $c.d $x) } {
                    param($c, $x)
                    $val = $(if ($c.d.type -eq 'int') { [long][math]::Round($x) } else { [double]$x })
                    Set-ModSetting $c.id @(@{ key = $c.d.key; value = $val })
                } $ctx
                $sl.Location = New-Object System.Drawing.Point(0, (Px 1))
                $p.Controls.Add($sl); $p.Controls.Add($vl)
                return $p
            }
            $nu = New-Object System.Windows.Forms.NumericUpDown
            $nu.Font = $fN; $nu.BackColor = $cField; $nu.ForeColor = $cText; $nu.Width = Px 120
            $nu.DecimalPlaces = $(if ($d.type -eq 'int') { 0 } else { 3 })
            $nu.Increment = $(if ($d.type -eq 'int') { 1 } elseif ($d.step) { [decimal]$d.step } else { [decimal]0.05 })
            $nu.Minimum = $(if ($null -ne $d.min) { [decimal]$d.min } else { [decimal]-1000000000 })
            $nu.Maximum = $(if ($null -ne $d.max) { [decimal]$d.max } else { [decimal]1000000000 })
            $nu.Value = [decimal][double]$v
            $nu.Tag = $ctx
            $nu.Add_Leave({ param($sx, $e2) $c = $sx.Tag; $val = $(if ($c.d.type -eq 'int') { [long]$sx.Value } else { [double]$sx.Value }); if (-not (Test-SameValue $val $c.cur)) { Set-ModSetting $c.id @(@{ key = $c.d.key; value = $val }) } })
            $ctx.cur = $v
            return $nu
        }
        'string' {
            $tb = New-Object System.Windows.Forms.TextBox
            $tb.Font = $fN; $tb.BackColor = $cField; $tb.ForeColor = $cText; $tb.Width = [math]::Min((Px 220), $room)
            if ($d.maxlen) { $tb.MaxLength = $d.maxlen }
            $tb.Text = [string]$v
            $ctx.cur = [string]$v
            $tb.Tag = $ctx
            $commit = { param($sx, $e2) $c = $sx.Tag; if ($sx.Text -cne $c.cur) { $c.cur = $sx.Text; Set-ModSetting $c.id @(@{ key = $c.d.key; value = [string]$sx.Text }) } }
            $tb.Add_Leave($commit)
            $tb.Add_KeyDown({ param($sx, $e2) if ($e2.KeyCode -eq 'Return') { $e2.SuppressKeyPress = $true; $c = $sx.Tag; if ($sx.Text -cne $c.cur) { $c.cur = $sx.Text; Set-ModSetting $c.id @(@{ key = $c.d.key; value = [string]$sx.Text }) } } })
            return $tb
        }
        'keybind' {
            $names = Get-BindList ([string]$v)
            $p = New-Object System.Windows.Forms.Panel
            $p.BackColor = $cCard
            $x = 0
            for ($slot = 0; $slot -lt $d.binds; $slot++) {
                $mine = ($S.modListen -and $S.modListen.id -ceq $id -and $S.modListen.key -ceq $d.key -and $S.modListen.slot -eq $slot)
                $txt = $(if ($mine) { 'Press a key...' } elseif ($slot -lt $names.Count) { Get-BindFriendly $names[$slot] } else { 'Not set' })
                $b = New-Button $txt $x 0 (Px 118) (Px 28) $mine
                if ($mine) { $b.Font = $fN }
                $b.Tag = @{ id = $id; d = $d; slot = $slot; names = $names; mine = $mine }
                $b.AccessibleName = "$($d.name), input $($slot + 1): $txt"
                $b.Add_Click({
                    param($sx, $e2)
                    if (([datetime]::Now - $S.listenAt).TotalMilliseconds -lt 400) { return }      # the Space that was just taken also clicks the button
                    $t = $sx.Tag
                    $S.modListen = $(if ($t.mine) { $null } else { @{ id = $t.id; key = $t.d.key; slot = $t.slot } })
                    Request-PageBuild 'mods'
                })
                if ($mine) {
                    $S.listenBtn = $b
                    $b.Add_PreviewKeyDown({ param($sx, $e2) $e2.IsInputKey = $true })
                    $b.Add_KeyDown({ param($sx, $e2) $e2.Handled = $true; $e2.SuppressKeyPress = $true; Set-ModBindInput $sx.Tag $(if ($e2.KeyCode -eq [System.Windows.Forms.Keys]::Escape) { '' } else { Get-BindNameOfKey $e2.KeyCode }) ($e2.KeyCode -eq [System.Windows.Forms.Keys]::Escape) })
                    $b.Add_MouseDown({
                        param($sx, $e2)
                        $n = switch ($e2.Button) { 'Right' { 'MouseRight' } 'Middle' { 'MouseMiddle' } 'XButton1' { 'MouseBack' } 'XButton2' { 'MouseForward' } default { $null } }
                        if ($n) { Set-ModBindInput $sx.Tag $n $false }
                    })
                    $b.Add_MouseWheel({ param($sx, $e2) Set-ModBindInput $sx.Tag $(if ($e2.Delta -gt 0) { 'WheelUp' } else { 'WheelDown' }) $false })
                }
                $p.Controls.Add($b)
                $x = $b.Right + (Px 6)
            }
            if ($names.Count -gt 0) {
                $ln = New-Link 'Clear' $x 0 (Px 44) (Px 28)
                $ln.TextAlign = 'MiddleLeft'; $ln.BackColor = $cCard; $ln.Tag = $ctx
                $ln.Add_LinkClicked({ param($sx, $e2) $c = $sx.Tag; Set-ModSetting $c.id @(@{ key = $c.d.key; value = '' }) })
                $p.Controls.Add($ln); $x = $ln.Right
            } else { $x += Px 44 }
            $p.Size = New-Object System.Drawing.Size($x, (Px 28))
            return $p
        }
    }
    return $null
}
function Set-ModBindInput($t, $name, [bool]$cancel) {
    $S.listenAt = [datetime]::Now
    if ($cancel) { $S.modListen = $null; Request-PageBuild 'mods'; return }
    if (-not $name) { return }      # a key the format has no name for: keep listening
    $list = New-Object System.Collections.Generic.List[string]
    foreach ($n in $t.names) { $list.Add([string]$n) }
    $at = [math]::Min($t.slot, $list.Count)
    if ($at -lt $list.Count) { $list[$at] = $name } else { $list.Add($name) }
    # the same input in another slot would be a double: it moves here
    $out = New-Object System.Collections.Generic.List[string]
    for ($i = 0; $i -lt $list.Count; $i++) { if ($i -eq $at -or $list[$i] -cne $name) { $out.Add($list[$i]) } }
    while ($out.Count -gt $t.d.binds) { $out.RemoveAt($out.Count - 1) }
    Set-ModSetting $t.id @(@{ key = $t.d.key; value = ($out -join ',') })
}
function Add-ModSettingRow($card, [int]$y, $e, $d, $cur) {
    $pad = Px 14; $inW = $card.Width - 2 * $pad
    $ctl = New-ModSettingControl $e $d $cur ($inW - (Px 200)) $inW
    if (-not $ctl) { return $y }
    $help = $d.desc
    if ($d.type -eq 'keybind') {
        $mine = Get-BindList ([string]$cur[$d.key])
        $also = @($e.sm.settings | Where-Object { $_.type -eq 'keybind' -and $_.key -cne $d.key -and $_.group -ceq $d.group } | Where-Object { $o = Get-BindList ([string]$cur[$_.key]); @($o | Where-Object { $mine -ccontains $_ }).Count -gt 0 } | ForEach-Object { $_.name })
        if ($also.Count -gt 0) { $help = ($help + "`r`nAlso used by $($also -join ', '): both react to the same input.").Trim() }
    }
    $r = Add-RowLayout $card $y $d.name $help $ctl
    return ($r.bottom + (Px 10))
}
function Add-ModSettingsPanel($card, [int]$cy, $e) {
    $pad = Px 14; $inW = $card.Width - 2 * $pad
    $sm = $e.sm
    $f = $S.modSet
    $n0 = $card.Controls.Count
    if ($f.newer) { $l = Add-Text $card 'settings.json was written by a newer version of the game. It is shown here but cannot be changed from this window.' $pad $cy $inW $fSmall $cAccent; $cy = $l.Bottom + (Px 6) }
    elseif ($f.broken) { $l = Add-Text $card "settings.json is damaged ($($f.broken)). The defaults are shown; the first change you make saves a new file and keeps the old one as settings.json.broken." $pad $cy $inW $fSmall $cAccent; $cy = $l.Bottom + (Px 6) }
    $cur = Get-ModValues $sm (Get-StoredModValues $e.id)
    # presets: "Default" (unless the mod has its own default preset) and the mod's presets; the one the values match is lit
    if ($sm.presets.Count -gt 0) {
        $hasDef = (@($sm.presets | Where-Object { $_.id -eq 'default' }).Count -gt 0)
        $labels = @(); if (-not $hasDef) { $labels += 'Default' }
        $labels += @($sm.presets | ForEach-Object { $_.name })
        $off = $(if ($hasDef) { 0 } else { 1 })
        $lit = Get-LitPreset $sm $cur
        $sel = $(if ($null -eq $lit) { -1 } elseif ($lit -eq -1) { 0 } else { $lit + $off })
        if ($null -ne $lit -and $lit -eq -1 -and $hasDef) { $sel = -1 }
        $ctx = @{ id = $e.id; sm = $sm; off = $off }
        $seg = New-Segmented ([string[]]$labels) $sel {
            param($c, $i)
            $eff = $(if ($i -lt $c.off) { Get-DefaultEffect $c.sm } else { Get-PresetEffect $c.sm $c.sm.presets[$i - $c.off] })
            Set-ModSetting $c.id @($eff | ForEach-Object { @{ key = $_.key; value = $_.value } })
        } $ctx
        $l = Add-Text $card 'Presets' $pad $cy $inW $fRow $cText; $cy = $l.Bottom + (Px 4)
        if ($seg.Width -le $inW) { $seg.Location = New-Object System.Drawing.Point($pad, $cy); $card.Controls.Add($seg); $cy = $seg.Bottom }
        else {
            $x = $pad
            for ($i = 0; $i -lt $labels.Count; $i++) {
                $w = (Measure-TextW $labels[$i] $fN) + (Px 26)
                if ($x + $w -gt $pad + $inW) { $x = $pad; $cy += Px 34 }
                $b = Add-PageButton $card $labels[$i] $x $cy @{ c = $ctx; i = $i } { param($sx, $e2) & $S.presetPick $sx.Tag.c $sx.Tag.i }
                if ($i -eq $sel) { $b.BackColor = $cAccent; $b.UseVisualStyleBackColor = $false; $b.Font = $fBtn }
                $x = $b.Right + (Px 6)
            }
            $cy += Px 30
        }
        if ($sel -lt 0) { $l = Add-Text $card 'Custom: your own mix of values.' $pad ($cy + (Px 2)) $inW $fSmall $cGrey; $cy = $l.Bottom }
        $cy += Px 12
    }
    foreach ($d in @($sm.settings | Where-Object { -not $_.advanced })) { $cy = Add-ModSettingRow $card $cy $e $d $cur }
    $adv = @($sm.settings | Where-Object { $_.advanced })
    if ($adv.Count -gt 0) {
        $open = [bool]$S.modAdv[$e.id]
        $ln = New-Link $(if ($open) { 'Hide advanced settings' } else { "Show advanced settings ($($adv.Count))" }) $pad $cy (Px 260) (Px 22)
        $ln.BackColor = $cCard; $ln.Tag = $e.id
        $ln.Add_LinkClicked({ param($sx, $e2) $S.modAdv[[string]$sx.Tag] = -not [bool]$S.modAdv[[string]$sx.Tag]; Request-PageBuild 'mods' })
        $card.Controls.Add($ln)
        $cy = $ln.Bottom + (Px 8)
        if ($open) { foreach ($d in $adv) { $cy = Add-ModSettingRow $card $cy $e $d $cur } }
    }
    $allDef = $true
    foreach ($d in $sm.settings) { if (-not (Test-SameValue $cur[$d.key] $d.default)) { $allDef = $false } }
    $b = Add-PageButton $card 'Reset to defaults' $pad $cy $e.id { param($sx, $e2) $x = Get-ModEntry ([string]$sx.Tag); if ($x) { Set-ModSetting $x.id @($x.sm.settings | ForEach-Object { @{ key = $_.key; remove = $true } }) } }
    $b.Enabled = (-not $allDef -and -not $f.newer)
    $l = Add-Text $card 'Changes apply the next time the game starts.' ($b.Right + (Px 10)) ($cy + (Px 6)) ($pad + $inW - $b.Right - (Px 10)) $fSmall $cGrey
    if ($f.newer) {
        # a settings.json of a newer game version is shown, never written: every control of this panel is greyed
        for ($i = $n0; $i -lt $card.Controls.Count; $i++) { $x = $card.Controls[$i]; if ($x -isnot [System.Windows.Forms.Label]) { $x.Enabled = $false } }
    }
    return ([math]::Max($b.Bottom, $l.Bottom) + (Px 4))
}
$S.presetPick = {
    param($c, $i)
    $eff = $(if ($i -lt $c.off) { Get-DefaultEffect $c.sm } else { Get-PresetEffect $c.sm $c.sm.presets[$i - $c.off] })
    Set-ModSetting $c.id @($eff | ForEach-Object { @{ key = $_.key; value = $_.value } })
}
function Build-ModsPage {
    $pg = $PageView.mods
    $S.mods = Read-ModStore
    $S.modSet = Read-ModSettingsFile
    $S.listenBtn = $null
    $c = New-PageContent $pg
    $c.SuspendLayout()
    $y = 0
    $y = Add-ModsToolbar $c $y
    $y = Add-ModsJobCard $c $y
    $y = Add-ModsMasterCard $c $y
    $ents = @($S.mods.entries)
    if ($ents.Count -eq 0) { if (-not $S.modJob) { $y = Add-ModsEmpty $c $y } }
    else {
        $disc = Get-DiscSerial
        $order = @($S.mods.active | ForEach-Object { Get-ModEntry $_ } | Where-Object { $_ })
        $order += @($ents | Where-Object { -not (Test-ModEnabled $_.id) })
        $scripts = @($order | Where-Object { $_.isScript }); $packs = @($order | Where-Object { -not $_.isScript })
        $both = ($scripts.Count -gt 0 -and $packs.Count -gt 0)
        foreach ($grp in @(@('Script mods', $scripts), @('Texture packs', $packs))) {
            if ($grp[1].Count -eq 0) { continue }
            if ($both) { $y = Add-GroupTitle $c ($y + (Px 2)) $grp[0] }
            foreach ($e in $grp[1]) { $y = Add-ModRow $c $y $e $disc }
        }
    }
    $c.Height = $y
    $c.ResumeLayout()
    Set-PageContent $pg $c
    if ($S.listenBtn) { try { $S.listenBtn.Focus() | Out-Null } catch { } }
}
function Open-ModsPage {
    $pg = $PageView.mods
    Set-PageNote $pg '' $null
    $pg.sub.Text = $pg.subText; $pg.sub.ForeColor = $cGrey
    if (Test-GameRunning) { $pg.sub.Text = 'The game is running. Close it to change mods: it uses these files while it runs.'; $pg.sub.ForeColor = $cAccent }
    $S.modListen = $null
    Build-ModsPage
    $pg.scroll.AutoScrollPosition = New-Object System.Drawing.Point(0, 0)
    try { $pg.scroll.Focus() | Out-Null } catch { }
}

# ================================================================ the two pages
$PageView.settings = New-PageView 'Settings' "The same options as the game's own Pause menu (Options). Each change is saved at once and used the next time the game starts."
$PageView.mods = New-PageView 'Mods' 'Script mods (like Arsenal Mastery, which comes with the game) and texture packs, including ones made for PCSX2. Changes apply the next time the game starts.'
$pSettings = $PageView.settings.panel; $UI.pSettings = $pSettings
$pMods = $PageView.mods.panel; $UI.pMods = $pMods
$PageView.settings.back.Add_LinkClicked({ Show-View $S.pageBack })
$PageView.mods.back.Add_LinkClicked({ if ($S.modJob -and $S.modJob.phase -in @('reading')) { Stop-ModJob }; Show-View $S.pageBack })
