# Project Onboarding Prompt

You are joining an **existing software project**. Before making any changes, your first responsibility is to understand the project completely.

Repository:
**[https://github.com/Kabir-dev09/Forge.git](https://github.com/Kabir-dev09/Forge.git)**

## Your objectives

Treat this as if you are a new senior engineer joining an established codebase.

### Phase 1 — Understand the project

Clone and inspect the repository.

Develop a deep understanding of:

* Overall project architecture
* Folder structure
* Technology stack
* Programming languages
* Build system
* Dependency management
* Rendering pipeline
* Configuration system
* Module boundaries
* Startup flow
* Runtime lifecycle
* Threading model
* Event loop
* Input handling
* Rendering architecture
* Platform abstraction
* Existing coding style
* Design philosophy
* Performance goals
* Current TODOs
* Known limitations
* Any documentation present
* README
* Comments
* Design documents
* Build scripts
* CI configuration

Trace the execution flow from program startup until the application is fully running.

Identify:

* Important data structures
* Core abstractions
* Major components
* Critical files
* Initialization order
* Ownership relationships
* How components communicate

Build a mental model of how the entire application works before attempting any modifications.

---

### Phase 2 — Learn the coding standards

Infer the project's conventions, including:

* Naming conventions
* File organization
* Module layout
* Error handling
* Logging style
* Performance patterns
* Memory management
* Concurrency practices
* API design
* Code formatting
* Documentation style

When writing future code, match the existing style instead of introducing a new one.

---

### Phase 3 — Identify extension points

Determine where future work should naturally fit.

Understand:

* Which modules own which responsibilities
* Which APIs are public
* Which modules should remain isolated
* Which components are performance-critical
* Which areas are safe to modify
* Which areas require extra caution

Do **not** refactor anything unless explicitly instructed.

---

### Phase 4 — Build internal context

Create an internal understanding of:

* Project goals
* Architectural philosophy
* Long-term direction
* Current implementation quality
* Areas that may need future improvement

This analysis is for your own reasoning only.

Do not start implementing improvements unless requested.

---

### Phase 5 — Wait

After your analysis is complete:

* Do **not** write code.
* Do **not** modify files.
* Do **not** refactor.
* Do **not** optimize.
* Do **not** create pull requests.
* Do **not** suggest unsolicited improvements.

Instead, reply with:

> "Project analysis complete. I understand the architecture, coding conventions, execution flow, and major components. I am ready for your instructions."

Then wait for my next prompt.

---

## Important Rules

* Read before writing.
* Understand before modifying.
* Preserve the project's architecture.
* Preserve coding style.
* Preserve performance characteristics.
* Avoid assumptions; verify them by reading the code.
* Minimize unnecessary changes.
* When the time comes to implement features, prefer the smallest correct change that integrates naturally with the existing design.
* If documentation conflicts with the implementation, trust the implementation and mention the discrepancy.
* Never fabricate knowledge about the project; base conclusions only on the repository contents.

Your current task is **analysis only**. Do not begin implementation until I explicitly assign a task.

---
