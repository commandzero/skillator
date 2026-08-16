# Skillator

Skillator manages which agent skills from a Library are active in one or more Skill Directories in a Target Repository.

## Language

**Library**:
A user's logical collection of Sources and their available Skills. Membership in the Library does not make a Skill active.
_Avoid_: Source, source directory, registry

**Source**:
A registered local directory that contributes one or more Skills to the Library. Sources may be Git-tracked or untracked, may contain several Skills, and need not share a common structure, so Skill discovery is recursive.
_Avoid_: Library, marketplace

**Skill**:
A directory supplied by a Source that contains a `SKILL.md` file.
_Avoid_: Package, plugin

**Target Repository**:
A Git repository whose active Skills Skillator manages. It is selected from the current working directory or an explicit directory argument.
_Avoid_: Source, skill directory, destination

**Skill Directory**:
An agent-specific directory inside a Target Repository where active Skills are materialized, such as `.agents/skills` or `.claude/skills`. A Target Repository may configure several Skill Directories with different Enablements.
_Avoid_: Target, library

**Enablement**:
The desired relationship that makes a Skill active in one specific Skill Directory. Its desired state may differ from the Materialization currently observed on disk.
_Avoid_: Installation, link

**Repository Configuration**:
The declarative `.agents/skillator.yml` file describing a Target Repository's Skill Directories and desired Enablements. It identifies Sources portably while user-level Library configuration resolves them to machine-specific paths.
_Avoid_: Library configuration, observed state

**Materialization**:
The filesystem representation of an Enablement: either Linked or Copied.
_Avoid_: Installation

**Linked**:
A Materialization represented by a symbolic link from a Skill Directory to the Skill in its Source. This is the default Materialization.

**Copied**:
A Materialization represented by a copy of the Skill in a Skill Directory. Changes in its Source require an explicit synchronization.
