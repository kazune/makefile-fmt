# .ONESHELL:
HELP = use your SHELL to run commands
define FOO
include foo.mk
.ONESHELL:
X=1
define BAR
Y=2
endef
endef

foo: ; echo    inline
foo:: bar
	echo    double
foo &: bar
	echo    grouped
foo: %.o: %.c
	echo    static
ifeq ($(X),1)
X=1
else
X=2
endif
unknown syntax
    space-indented text

all:
	@echo hi # shell comment
	@cat <<EOF
	@printf '%s\n' 'a\
	b'
	@echo "unclosed
	@for
