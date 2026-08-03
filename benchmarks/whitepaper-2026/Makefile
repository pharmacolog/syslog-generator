SHELL := /bin/bash
.SHELLFLAGS := -eu -o pipefail -c
.DEFAULT_GOAL := help

REQUIRE_TOOLS ?= 0
SKIP_BUILD    ?= 0
PRESERVE_RESULTS ?= 0
DURATION_SECS ?= 30
RUNS          ?= 1
WORKLOADS     ?=
TOOLS         ?=

SCRIPT_DIR     := $(abspath $(dir $(lastword $(MAKEFILE_LIST))))
WHITEPAPER_DIR := $(SCRIPT_DIR)
REPO_ROOT      := $(abspath $(SCRIPT_DIR)/../..)
CONFIGS_DIR    := $(SCRIPT_DIR)/configs
SCRIPTS_DIR    := $(SCRIPT_DIR)/scripts
RESULTS_DIR    := $(SCRIPT_DIR)/results
PERF_DIR       := $(REPO_ROOT)/perf
WP_RESULTS     := $(PERF_DIR)/whitepaper-results.json
WP_REPORT      := $(RESULTS_DIR)/REPORT.md
HARNESS_DIR    := $(SCRIPT_DIR)/harness

SYSLOG_GENERATOR_BIN := $(REPO_ROOT)/target/release/syslog-generator
LOGGEN_BIN  := $(shell command -v loggen 2>/dev/null || echo "")
FLOG_BIN    := $(shell command -v flog 2>/dev/null || echo "")
TCPKALI_BIN := $(shell command -v tcpkali 2>/dev/null || echo "")

ifneq ($(shell [ -t 1 ] && echo tty),)
    C_RED    := \033[0;31m
    C_GREEN  := \033[0;32m
    C_YELLOW := \033[0;33m
    C_BLUE   := \033[0;34m
    C_BOLD   := \033[1m
    C_OFF    := \033[0m
else
    C_RED    :=
    C_GREEN  :=
    C_YELLOW :=
    C_BLUE   :=
    C_BOLD   :=
    C_OFF    :=
endif

export REQUIRE_TOOLS SKIP_BUILD PRESERVE_RESULTS DURATION_SECS RUNS WORKLOADS TOOLS
export SCRIPT_DIR WHITEPAPER_DIR REPO_ROOT CONFIGS_DIR SCRIPTS_DIR RESULTS_DIR PERF_DIR
export WP_RESULTS WP_REPORT HARNESS_DIR
export SYSLOG_GENERATOR_BIN LOGGEN_BIN FLOG_BIN TCPKALI_BIN
export C_RED C_GREEN C_YELLOW C_BLUE C_BOLD C_OFF

.PHONY: help all dry-run validate report check-tools harness-tests clean \
        build-syslog-generator reset \
        run-syslog-generator run-loggen run-flog run-tcpkali

help:
	@printf "$(C_BOLD)Whitepaper 2026 — benchmark harness$(C_OFF)\n"
	@printf "Issue #106 / GTM-1\n"
	@printf "\n"
	@printf "$(C_BOLD)Targets:$(C_OFF)\n"
	@printf "  $(C_GREEN)make all$(C_OFF)                Dry-run (default). Safe; never fails.\n"
	@printf "  $(C_GREEN)make all REQUIRE_TOOLS=1$(C_OFF) Real run. Requires installed tools.\n"
	@printf "  $(C_GREEN)make validate$(C_OFF)          Validate configs + harness tests.\n"
	@printf "  $(C_GREEN)make report$(C_OFF)            Regenerate REPORT.md from $(WP_RESULTS).\n"
	@printf "  $(C_GREEN)make harness-tests$(C_OFF)     Run self-tests only.\n"
	@printf "  $(C_GREEN)make check-tools$(C_OFF)       Detect available tools + host.\n"
	@printf "  $(C_GREEN)make clean$(C_OFF)             Remove results/ + perf/whitepaper-results.json.\n"
	@printf "\n"
	@printf "$(C_BOLD)Variables:$(C_OFF)\n"
	@printf "  REQUIRE_TOOLS=0|1   default: 0 (dry-run)\n"
	@printf "  SKIP_BUILD=0|1      default: 0\n"
	@printf "  RUNS=N              default: 1\n"
	@printf "  DURATION_SECS=N     default: 30\n"
	@printf "  RATE_TOLERANCE_FRACTION=N  default: 0.05 (95–105%)\n"
	@printf "  WORKLOADS='w1 w2'   default: all 4\n"
	@printf "  TOOLS='t1 t2'       default: all 4 (syslog_generator loggen flog tcpkali)\n"

all:
	@printf "$(C_GREEN)[OK] make all complete (REQUIRE_TOOLS=$(REQUIRE_TOOLS))$(C_OFF)\n"

check-tools:
	@printf "$(C_BOLD)== check-tools ==$(C_OFF)\n"
	@bash $(SCRIPTS_DIR)/01_check_tools.sh > /tmp/.check_tools.stdout 2> /tmp/.check_tools.stderr
	@if command -v jq >/dev/null 2>&1; then \
		jq -r '.tool_versions | to_entries[] | "  \(.key): available=\(.value.available) path=\(.value.path // "-")"' /tmp/.check_tools.stdout; \
	else \
		cat /tmp/.check_tools.stdout; \
	fi

validate:
	@printf "$(C_BOLD)== validate ==$(C_OFF)\n"
	@python3 $(CONFIGS_DIR)/validate.py
	@printf "$(C_GREEN)[OK] all config validations passed$(C_OFF)\n"

harness-tests:
	@printf "$(C_BOLD)== harness-tests ==$(C_OFF)\n"
	@bash $(HARNESS_DIR)/test_harness.sh
	@python3 $(HARNESS_DIR)/test_collect.py
	@python3 $(HARNESS_DIR)/test_receiver.py
	@printf "$(C_GREEN)[OK] all harness tests passed$(C_OFF)\n"

dry-run:
	@printf "$(C_BOLD)== dry-run ==$(C_OFF)\n"
	@bash $(SCRIPTS_DIR)/02_dry_run.sh

dispatch:
	@printf "$(C_BOLD)== dispatch (REQUIRE_TOOLS=$(REQUIRE_TOOLS)) ==$(C_OFF)\n"
	@bash $(SCRIPTS_DIR)/03_dispatch.sh

ifneq ($(REQUIRE_TOOLS),0)
dispatch: build-syslog-generator
endif

collect:
	@printf "$(C_BOLD)== collect ==$(C_OFF)\n"
	@python3 $(SCRIPTS_DIR)/04_collect.py $(RUNS)

report:
	@printf "$(C_BOLD)== report ==$(C_OFF)\n"
	@python3 $(SCRIPTS_DIR)/05_report.py $(WP_REPORT)

ifneq ($(REQUIRE_TOOLS),0)
all: check-tools validate harness-tests dry-run dispatch collect report
else
all: check-tools validate harness-tests dry-run dispatch collect report
endif

build-syslog-generator: $(SYSLOG_GENERATOR_BIN)

$(SYSLOG_GENERATOR_BIN):
	@printf "$(C_BOLD)== build syslog-generator ==$(C_OFF)\n"
	@if [[ "$(SKIP_BUILD)" == "1" ]]; then \
		printf "$(C_RED)[ERROR] $(SYSLOG_GENERATOR_BIN) not found and SKIP_BUILD=1$(C_OFF)\n"; \
		exit 1; \
	else \
		(cd $(REPO_ROOT) && cargo build --release --locked --bin syslog-generator); \
	fi

run-syslog-generator: validate
	@bash $(SCRIPTS_DIR)/03_run_syslog_generator.sh $(word 2,$(MAKECMDGOALS)) 1
run-loggen: validate
	@bash $(SCRIPTS_DIR)/03_run_loggen.sh $(word 2,$(MAKECMDGOALS)) 1
run-flog: validate
	@bash $(SCRIPTS_DIR)/03_run_flog.sh $(word 2,$(MAKECMDGOALS)) 1
run-tcpkali: validate
	@bash $(SCRIPTS_DIR)/03_run_tcpkali.sh $(word 2,$(MAKECMDGOALS)) 1

clean:
	@printf "$(C_YELLOW)== clean ==$(C_OFF)\n"
	@if [[ "$(PRESERVE_RESULTS)" == "1" ]]; then \
		printf "$(C_YELLOW)[SKIP] PRESERVE_RESULTS=1, not cleaning$(C_OFF)\n"; \
	else \
		rm -rf $(RESULTS_DIR)/*; \
		printf "$(C_GREEN)[OK] results cleaned$(C_OFF)\n"; \
	fi
	@printf "$(C_YELLOW)Note: $(WP_RESULTS) preserved (it is the schema artifact)$(C_OFF)\n"

reset:
	@printf "$(C_BOLD)== reset to schema_only ==$(C_OFF)\n"
	@python3 $(SCRIPTS_DIR)/reset_schema.py $(WP_RESULTS)

%:
	@true
