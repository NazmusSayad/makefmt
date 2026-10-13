a := "1"
b := if a == "1" { "x" } else { "y" }
c := if a != "1" { "x" } else if a =~ "^[0-9]+$" { "y" } else if a !~ 'z' { "w" } else { "v" }
d := if a=="1"{"x"}else{"y"}
e := if a == "1" {
"x" } else { "y" }
