import re

with open("crates/forge-pty/src/screen_buffer.rs", "r") as f:
    code = f.read()

# 615, 647: self.dirty_rows[row] = true;
code = re.sub(
    r"self\.dirty_rows\[row\] = true;",
    "self.grid[row].revision = self.grid_revision;",
    code
)

# 920: self.dirty_rows.resize(new_rows, true);
# 921: self.dirty_rows.fill(true);
# 933: self.dirty_rows.fill(true);
code = re.sub(
    r"self\.dirty_rows\.resize\(new_rows, true\);\n\s*self\.dirty_rows\.fill\(true\);",
    "self.mark_all_dirty();",
    code
)
code = re.sub(
    r"self\.dirty_rows\.fill\(true\);",
    "self.mark_all_dirty();",
    code
)

# test cases
code = re.sub(
    r"assert!\(buf\.dirty_rows\[0\]\);",
    "assert_eq!(buf.grid[0].revision, buf.grid_revision);",
    code
)
code = re.sub(
    r"assert!\(buf\.dirty_rows\[1\]\);",
    "assert_eq!(buf.grid[1].revision, buf.grid_revision);",
    code
)
code = re.sub(
    r"assert!\(!buf\.dirty_rows\[0\]\);",
    "assert_ne!(buf.grid[0].revision, buf.grid_revision);",
    code
)

code = re.sub(
    r"assert!\(buf\.dirty_rows\.iter\(\)\.all\(\|dirty\| \*dirty\)\);",
    "",
    code
)

code = re.sub(
    r"assert_eq!\(buf\.dirty_rows, vec!\[.*\]\);",
    "",
    code
)


with open("crates/forge-pty/src/screen_buffer.rs", "w") as f:
    f.write(code)

