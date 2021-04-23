"""Setup"""

from distutils.core import setup

setup(
    name="kworkspaces",
    version="0.1.0",
    packages=["kworkspaces"],
    entry_points="""
  [console_scripts]
  kworkspaces = kworkspaces:run
  """,
)
