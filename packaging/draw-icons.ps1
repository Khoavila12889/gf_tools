$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing
$out = Join-Path (Split-Path $PSScriptRoot -Parent) 'rust\launcher\assets'
New-Item -ItemType Directory -Path $out -Force | Out-Null
# Original line artwork. Black is the native image-list transparency key.
function Line($x1,$y1,$x2,$y2) { $script:g.DrawLine($script:pen,[single]$x1,[single]$y1,[single]$x2,[single]$y2) }
function Rect($x,$y,$w,$h) { $script:g.DrawRectangle($script:pen,[single]$x,[single]$y,[single]$w,[single]$h) }
function Circle($x,$y,$w,$h) { $script:g.DrawEllipse($script:pen,[single]$x,[single]$y,[single]$w,[single]$h) }
foreach($name in @('unlock','cpu','disk','sliders','document','shield','refresh','trash','app-minus')) {
    $bmp = New-Object System.Drawing.Bitmap 32,32
    $script:g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.Clear([System.Drawing.Color]::Black)
    $script:pen = New-Object System.Drawing.Pen ([System.Drawing.Color]::FromArgb(40,93,145)),2
    switch($name) {
        unlock { Rect 7 14 18 13; $g.DrawArc($pen,11,3,13,17,180,180); Line 11 11 11 14; Line 16 19 16 23 }
        cpu { Rect 8 8 16 16; Rect 12 12 8 8; foreach($v in @(11,16,21)){Line $v 3 $v 7; Line $v 25 $v 29; Line 3 $v 7 $v; Line 25 $v 29 $v} }
        disk { Rect 5 5 22 22; Circle 9 8 14 14; Circle 14 13 4 4; Line 16 16 24 23; Line 8 24 11 24 }
        sliders { foreach($v in @(8,16,24)){Line 4 $v 28 $v}; Rect 9 5 4 6; Rect 20 13 4 6; Rect 12 21 4 6 }
        document { Line 7 3 20 3; Line 20 3 26 9; Line 26 9 26 29; Line 26 29 7 29; Line 7 29 7 3; Line 20 3 20 9; Line 20 9 26 9; Line 11 14 22 14; Line 11 19 22 19; Line 11 24 19 24 }
        shield { Line 16 3 27 7; Line 27 7 26 19; Line 26 19 22 25; Line 22 25 16 29; Line 16 29 10 25; Line 10 25 6 19; Line 6 19 5 7; Line 5 7 16 3; Line 10 16 14 20; Line 14 20 23 11 }
        refresh { $g.DrawArc($pen,5,5,22,22,210,145); $g.DrawArc($pen,5,5,22,22,30,145); Line 27 6 27 14; Line 27 14 19 14; Line 5 26 5 18; Line 5 18 13 18 }
        trash { Line 5 8 27 8; Rect 12 4 8 4; Line 8 10 10 28; Line 10 28 23 28; Line 23 28 25 10; Line 13 12 14 24; Line 20 12 19 24 }
        app-minus { Rect 4 5 24 23; Line 4 11 28 11; Line 8 8 9 8; Line 12 8 13 8; Line 10 20 22 20 }
    }
    $bmp.Save((Join-Path $out "$name.bmp"),[System.Drawing.Imaging.ImageFormat]::Bmp)
    $pen.Dispose(); $g.Dispose(); $bmp.Dispose()
}
