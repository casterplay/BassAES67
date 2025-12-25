# Development Guidelines

This document outlines development principles for building features systematically and maintainably.

## Core Principles

### One Feature at a Time
- Work on exactly one feature per development cycle
- If scope is unclear from the prompt, ask clarifying questions rather than making assumptions
- Complete current feature entirely before starting the next one
- Avoid adding "nice to have" functionality during feature development

### Simplicity First
- Prefer simple, readable code over clever abstractions
- Avoid premature optimization and complex inheritance structures
- Add complexity only when it genuinely reduces code size or improves maintainability
- Keep functions/methods focused and concise
- Touch as few files as possible per feature (ideally 1-2 files)

## Feature Development Process

### Scope Definition
- Ask questions to clarify feature boundaries when unclear
- Break large features into smaller, independent sub-features when appropriate
- Define clear completion criteria before starting

### Implementation Approach
- Start with the simplest working solution
- Refactor for clarity, not cleverness
- Minimize file changes and cross-cutting modifications
- Exception: Interface changes require updating all references

### .NET Project Guidelines
- In C#, avoid Interfaces just for the sake of it. Only use Interfaces when needed.
- Use .NET framework is version 10.
- "Nullabe" is on by default, code recording to that.
- When working with Web UI in Blazor use MudBlazor components.
- When working a Dialogs in MudBlazor place the razor file in foler: Components > Dialogs. Declare it like this: [CascadingParameter] IMudDialogInstance? MudDialog { get; set; } 
- When creating database from classes use: "dotnet ef migrations add InitialCreate" + "dotnet ef database update"
- When updating database from classes use: "dotnet ef migrations add 'name of update'" + "dotnet ef database update"
- Use service injection to camelCase naming convention, like this: @inject DeviceService deviceService
- For databse EF, keep "QueryTrackingBehavior.NoTracking". Create new database querys if needed for add and update. Never use Tracking or "AsTracking"
- Do not use GUID or a string of GUID as unique identifier in database classes, use: "[Key] public int Id { get; set; }"
- Never use "object" in C#!
- Never use "unsafe" code in C#

### Rust Project Guidelines
- Never use MUTEX in an audio application!

### Dependency Management
- Ask before adding new dependencies
- Explain why the dependency is needed
- When multiple viable options exist, present choices with brief descriptions and examples
- Prefer existing project dependencies when possible

## Code Quality Standards

### Documentation
- Add brief comments to all functions/methods/classes explaining their purpose
- Avoid excessive commenting unless code behavior is non-obvious
- No additional documentation unless specifically requested

### Code Style
- Prioritize readability over conciseness
- Use clear, descriptive names
- Keep functions small and focused
- Avoid nested complexity when possible

## Interface and Cascading Changes

When modifying interfaces, APIs, or shared contracts:
- Update all references in the same development cycle
- Ensure consistency across all affected files
- Test compilation/syntax after interface changes

## Feature Completion Process

1. Implement feature according to requirements
2. Ensure all affected files are updated and consistent
3. Present completed feature to user for review
4. Wait for explicit user acknowledgment of completion
5. Upon acknowledgment, create git commit with auto-generated descriptive message

## Testing

Currently no testing requirements. This section can be updated as project needs evolve.

## Project-Specific Extensions

This file serves as a foundation. Additional project-specific guidelines can be added below this section as needed.