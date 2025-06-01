# codefact

This is a small, over-engineered Rust command-line tool for keeping you logged into AWS CodeArtifact, with a fresh auth token in your environment. It writes some shell files for you to source for this purpose. It is another tool in a long series of tools that solve extremely specific problems that nobody else has.

## Rationale

I am both extremely lazy and extremely absent-minded. An eight-hour token expiration is far too frequent for me to keep track of.

## Usage

`codefact`'s only configuration is through environment variables. You tell your shell how you use CodeArtifact and AWS through some standard variables, and the tool consumes them. If you provide the optional python and maven repo names, the tool writes out configuration for using those repos with `uv` and `mvn` respectively.

Environment variables read:

* `AWS_DOMAIN`: the domain name your CodeArtifact repositories are under. This is re-used as the repo id for maven.
* `AWS_ACCOUNT_ID`: the numeric AWS account ID
* `AWS_REGION`: the AWS region to use in the CodeArtifact repo URI.
* `CODEARTIFACT_PYTHON_REPO` The name of your python repository, if you use one. Writes `uv`'s config variables for bash and fish shells if set.
* `CODEARTIFACT_MAVEN_REPO`: The name of your maven repository, if you use one. Writes `~/.m2/settings.xml` if set. It replaces only the password line for the codeartifact repo if the line contains a magic cookie.

Environment variables set:

* `AWS_CODEARTIFACT_TOKEN`: the auth token.
* `CODEARTIFACT_TOKEN_EXPIRY`: milliseconds since the unix epoch when the token will expire.
* `UV_DEFAULT_INDEX`: for `uv`'s use, if you have set a python repo name
* `UV_PUBLISH_URL`: for `uv`'s use, if you have set a python repo name
* `UV_PUBLISH_PASSWORD`: for `uv`'s use, if you have set a python repo name

If you run it in an interactive context (aka a tty), it prints to stderr either the remaining time before an existing token expires, or instructions for how to load a new token into your environemtn. If you run it in a non-interactive context, it prints a good token to stdout and nothing to stderr. This makes it suitable for use in shell scripts, should you have such an odd use case. Perhaps you are me?

## TODO

- Support `zsh` even though I don't use it. Heck, how about `elvish` too?

## LICENSE

This code is licensed via [the Parity Public License.](https://paritylicense.com) This license requires people and companies who build on top of this source code to share their work with the community, too. See the license text for details.
