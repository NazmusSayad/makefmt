default:
  #!powershell.exe
  $foo = 'café'
  if ($foo.Length -ne 4) { exit 1 }
