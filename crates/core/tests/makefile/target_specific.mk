all: A = 1
all : B := 2
build:   export C += -O2
%.o: CFLAGS += -g
%.o :   override D = x
lib.a: private E ?= y
all: F = a:b c=d
s1.o s2.o: G = 1
install: unexport H = 2
all: I = trailing  
