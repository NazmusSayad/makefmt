ifdef MISSING
	CFLAGS += -g
else
	CFLAGS += -O2
endif
	LDFLAGS = -lm   
all:
	echo $(CFLAGS)
