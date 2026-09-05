CC=gcc
CFLAGS  :=   -O2

all:
	@echo    one
	@-+if true; then \
	echo yes; \
	fi
	@printf '%s\n' ok
