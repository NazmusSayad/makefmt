set windows-shell := ['pwsh.exe', '-NoLogo', '-Command']

foo := shell('Write-Output bar')

default:
  @echo {{foo}}
