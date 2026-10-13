set shell:=["bash","-c"]
export A:="1"
B:=A+"2"
alias c:=d
# doc
[private]
@d e f="g" +h:c2 (c2)&&c2
  echo {{e}}



c2:
	echo
