# Compiler
CC=gcc
# Flags, with a long name
CFLAGS_FOR_RELEASE:=-O2

# Separate group after a blank line
X=1

ifdef DEBUG
# inside a conditional
A=1
  # indented comment
LONGER=2
endif
all:
	echo hi
