unexport A  =  1   spaced   value  
unexport define U
  keep   indentation
endef
a& : b
c&:d ; touch c
SEMI := ;
semi: $(SEMI) echo "a   b"
private  p : ; echo "p   p"
override : ; echo "o   o"
s.o: %.o: %.c:x
t.o: %.o: X=1
u.o: %.o::x
v.o:%.o:%.c   w.c
a\=b   =   c
d\$:=#=
e: \$(info a  b)
f: x$ 
g:   $	
h\&: i ; @true
.SECONDEXPANSION:
second: $$(subst a,  b,x)   $$@.c
