from setuptools import setup, find_packages

setup(
    name="aimp-client",
    version="0.1.0",
    description="Python client SDK for the AI Mesh Protocol (AIMP)",
    packages=find_packages(),
    python_requires=">=3.10",
    install_requires=[
        "msgpack>=1.0.8",
        "pynacl>=1.5.0",
    ],
    entry_points={
        "console_scripts": [
            "aimp-cli=aimp_client.cli:main",
        ],
    },
)
