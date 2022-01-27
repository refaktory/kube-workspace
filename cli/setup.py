"""Setup"""

from distutils.core import setup

setup(
    name="kworkspace",
    version="0.1.0",
    packages=["kworkspace"],
    entry_points="""
  [console_scripts]
  kworkspaces = kworkspace:run
  """,
)
