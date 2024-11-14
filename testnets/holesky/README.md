# Holesky Launch Instructions

This document provides instructions for running the Bolt sidecar on the Holesky testnet.

# Table of Contents

<!-- vim-markdown-toc GFM -->

* [Prerequisites](#prerequisites)
* [On-Chain Registration](#on-chain-registration)
  * [Validator Registration](#validator-registration)
    * [Registration Steps](#registration-steps)
  * [Bolt Network Entrypoint](#bolt-network-entrypoint)
  * [Operator Registration](#operator-registration)
    * [Symbiotic Registration Steps](#symbiotic-registration-steps)
    * [EigenLayer Registration Steps](#eigenlayer-registration-steps)
* [Off-Chain Setup](#off-chain-setup)
  * [Docker Mode (recommended)](#docker-mode-recommended)
  * [Commit-Boost Mode](#commit-boost-mode)
  * [Native Mode (advanced)](#native-mode-advanced)
    * [Building and running the MEV-Boost fork binary](#building-and-running-the-mev-boost-fork-binary)
    * [Building and running the Bolt sidecar binary](#building-and-running-the-bolt-sidecar-binary)
      * [Configuration file](#configuration-file)
    * [Observability](#observability)
* [Reference](#reference)
  * [Command-line options](#command-line-options)
  * [Delegations and signing options for Native and Docker Compose Mode](#delegations-and-signing-options-for-native-and-docker-compose-mode)
    * [`bolt` CLI](#bolt-cli)
      * [Installation and usage](#installation-and-usage)
    * [Using a private key directly](#using-a-private-key-directly)
    * [Using a ERC-2335 Keystore](#using-a-erc-2335-keystore)
  * [Avoid restarting the beacon node](#avoid-restarting-the-beacon-node)

<!-- vim-markdown-toc -->

<!-- Links -->

[bolt]: https://docs.boltprotocol.xyz
[constraints-api]: https://docs.boltprotocol.xyz/technical-docs/api/builder

# Prerequisites

In order to run Bolt you need some components already installed and running on
your system.

**A synced Geth client:**

Bolt is fully trustless since it is able to produce a fallback block with the
commitments issued in case builders do not return a valid bid. In order to do so
it relies on a synced execution client, configured via the `--execution-api-url`
flag. **At the moment only Geth is supported; with more
clients to be supported in the future.**

Using the sidecar with a different execution client could lead to commitment
faults because fallback block building is not supported yet. You can download
Geth from [the official website](https://geth.ethereum.org/downloads).

**A synced beacon node:**

Bolt is compatible with every beacon client. Please refer to the various beacon
client implementations to download and run them.

> [!IMPORTANT]
> In order to correctly run the Bolt sidecar and avoid commitment faults the
> beacon node must be configured so that:
>
> 1. the node's `builder-api` (or equivalent flag) must point to the Bolt
>    Sidecar API.
> 2. the node will always prefer the builder payload. For instance, in
>    Lighthouse this can be achieved by providing the following flags:
>
>    ```text
>    --always-prefer-builder-payload
>    --builder-fallback-disable-checks
>    ```
>
> In other clients like Vouch, the same can be achieved by setting the
> `builder-boost-factor` to a large value like `18446744073709551615` (`2**64 -
1`).
>
> It might be necessary to restart your beacon node depending on your existing
> setup. See the [Avoid Restarting the Beacon
> Node](#avoid-restarting-the-beacon-node) section for more details.

**Active validators:**

The Bolt sidecar requires access to BLS signing keys from active Ethereum validators,
or **authorized delegates** acting on their behalf, to issue and sign preconfirmations.

To learn more about delegation, check out the [Delegations and Signing](#delegations-and-signing-options-for-native-and-docker-compose-mode)
section.

> [!NOTE]
> Before moving on to the actual instructions, please note that the on-chain steps must be completed before running the off-chain
> infrastructure. The sidecar will verify that all of the associated validators and operator have been registered in the Bolt contracts,
> else it will fail (for safety reasons).

# On-Chain Registration

The first step for integrating Bolt is registering into the Bolt smart contracts. This is required for signalling
participation, depositing collateral, and specifying endpoints and other metadata to start receiving preconfirmation
requests. What follows is a quick overview of the required steps.

First you'll need to deposit some collateral in the form of whitelisted ETH derivative tokens that need to
be restaked in either the Symbiotic or EigenLayer restaking protocols. Bolt is compatible with the following ETH derivative tokens on Holesky:

- [Symbiotic Vaults](https://docs.symbiotic.fi/deployments/current#vaults)
  - [`wstETH`](https://holesky.etherscan.io/address/0xc79c533a77691641d52ebD5e87E51dCbCaeb0D78)
  - [`rETH`](https://holesky.etherscan.io/address/0xe5708788c90e971f73D928b7c5A8FD09137010e0)
  - [`stETH`](https://holesky.etherscan.io/address/0x11c5b9A9cd8269580aDDbeE38857eE451c1CFacd)
  - [`wETH`](https://holesky.etherscan.io/address/0xC56Ba584929c6f381744fA2d7a028fA927817f2b)
  - [`cbETH`](https://holesky.etherscan.io/address/0xcDdeFfcD2bA579B8801af1d603812fF64c301462)
  - [`mETH`](https://holesky.etherscan.io/address/0x91e84e12Bb65576C0a6614c5E6EbbB2eA595E10f)
- [EigenLayer Strategies](https://github.com/Layr-Labs/eigenlayer-contracts#current-testnet-deployment)
  - [`stETH`](https://holesky.etherscan.io/address/0x3F1c547b21f65e10480dE3ad8E19fAAC46C95034)
  - [`rETH`](https://holesky.etherscan.io/address/0x7322c24752f79c05FFD1E2a6FCB97020C1C264F1)
  - [`wETH`](https://holesky.etherscan.io/address/0x94373a4919B3240D86eA41593D5eBa789FEF3848)
  - [`cbETH`](https://holesky.etherscan.io/address/0x8720095Fa5739Ab051799211B146a2EEE4Dd8B37)
  - [`mETH`](https://holesky.etherscan.io/address/0xe3C063B1BEe9de02eb28352b55D49D85514C67FF)

> [!NOTE]
> These Vaults and Strategies have been deployed on Holesky by us, and are permissionless to opt in to.
> For now, these are the only vaults & strategies that have been whitelisted by the Bolt protocol.

After that, you need to interact with two contracts on Holesky:
`BoltValidators` and `BoltManager`. The former is used to register your
active validators into the protocol, while the latter is used to
register as an operator into the system and integrate with the restaking
protocols.

> [!IMPORTANT]
> When registering your operator in the `BoltManager` contract you must use the
> public key associated to the private key used to sign commitments with the
> Bolt Sidecar (the `--commitment-private-key` flag).

**Prerequisites**

- Install the Foundry toolkit:

```bash
curl -L https://foundry.paradigm.xyz | bash
source $HOME/.bashrc
foundryup
```

- Clone the Bolt repo and navigate to the [contracts](https://github.com/chainbound/bolt/tree/unstable/bolt-contracts) directory:

```bash
git clone https://github.com/chainbound/bolt
cd bolt-contracts
forge install
```

## Validator Registration

The [`BoltValidators`](../../bolt-contracts/src/contracts/BoltValidatorsV1.sol) contract is the only
point of entry for validators to signal their intent to participate in Bolt
Protocol and authenticate with their BLS private key (unsupported until Pectra).

The registration process includes the following steps:

1. Validator signs a message with their BLS private key. This is required to
   prove that the validator private key is under their control and that they are
   indeed its owner.
2. Validator calls the `registerValidator` function providing:
   1. Their BLS public key
   2. The BLS signature of the registration message
   3. The address of the authorized collateral provider & operator (commitment signer)

Until the Pectra hard-fork will be activated, the contract will also expose a
`registerValidatorUnsafe` function that will not check the BLS signature. This
is gated by a feature flag that will be turned off post-Pectra and will allow us
to test the registration flow in a controlled environment.

Note that the account initiating the registration will be the `controller`
account for those validators. Only the `controller` can then deregister
validator or change any preferences.

### Registration Steps

> [!NOTE]
> All of these scripts can be simulated on a Holesky fork using Anvil with the
> following command:
>
> `anvil --fork-url https://holesky.drpc.org --port 8545`
>
> In order to use this local fork, replace `$HOLESKY_RPC` with `localhost:8545` in
> all of the `forge` commands below.

To register your validators, we provide the following Foundry script:
[`RegisterValidators.s.sol`](../../bolt-contracts/script/holesky/validators/RegisterValidators.s.sol).
Note that in order to run these scripts, you must be in the `bolt-contracts`
directory.

- First, configure
  [`bolt-contracts/config/holesky/validators.json`](../../bolt-contracts/config/holesky/validators.json)
  to your requirements. Note that both `maxCommittedGasLimit` and
  `authorizedOperator` must reflect the values you'll specify in later steps, during
  the configuration of the sidecar. `pubkeys` should be configured with all of the
  validator public keys that you wish to register.

- Next up, decide on a controller account and save the key in an environment
  variable: `export CONTROLLER_KEY=0x...`. This controller key will be used to run
  the script and will mark the corresponding account as the [controller
  account](https://github.com/chainbound/bolt/blob/06bdd8e75d759d91f6178ad73f962b1f4ad43fd8/bolt-contracts/src/interfaces/IBoltValidatorsV1.sol#L18-L19)
  for these validators.

- Finally, run the script:

```bash
forge script script/holesky/validators/RegisterValidators.s.sol -vvvv --rpc-url $HOLESKY_RPC --private-key $CONTROLLER_KEY --broadcast
```

If the script executed succesfully, your validators were registered.

## Bolt Network Entrypoint

The [`BoltManager`](../../bolt-contracts/src/contracts/BoltManagerV1.sol)
contract is a crucial component of Bolt that integrates with restaking
ecosystems Symbiotic and Eigenlayer. It manages the registration and
coordination of validators, operators, and vaults within the Bolt network.

Key features include:

1. Retrieval of operator stake and proposer status from their pubkey
2. Integration with Symbiotic
3. Integration with Eigenlayer

Specific functionalities about the restaking protocols are handled inside the
`IBoltMiddleware` contracts, such as [`BoltSymbioticMiddleware`](../../bolt-contracts/src/contracts/BoltSymbioticMiddlewareV2.sol) and
[`BoltEigenlayerMiddleware`](../../bolt-contracts/src/contracts/BoltEigenLayerMiddlewareV2.sol).

## Operator Registration

In this section we outline how to register as an operator, i.e. an entity
uniquely identified by an Ethereum address and responsible for duties like
signing commitments. Note that in Bolt, there is no real separation between
validators and an operator. An operator is only real in the sense that its
private key will be used to sign commitments on the corresponding validators'
sidecars. However, we need a way to logically connect validators to an on-chain
address associated with some stake, which is what the operator abstraction takes care of.

**In the next sections we assume you have saved the private key corresponding to
the operator address in `$OPERATOR_SK`.** This private key will be read by the
Forge scripts for registering operators and needs to be set correctly. You also
have to invoke the scripts from the [`bolt-contracts`](../../bolt-contracts)
directory.

### Symbiotic Registration Steps

As an operator, you will need to opt-in to the Bolt Network and any Vault that
trusts you to provide commitments on their behalf.

**External Steps**

> [!NOTE]
> The network and supported vault addresses can be found in
> [`deployments.json`](../../bolt-contracts/config/holesky/deployments.json).

Make sure you have installed the [Symbiotic
CLI](https://docs.symbiotic.fi/guides/cli/).

The opt-in process requires the following steps:

1. if you haven't done it already, register as a Symbiotic Operator with the
   [`register-operator`](https://docs.symbiotic.fi/guides/cli/#register-operator)
   command;
2. opt-in to the Bolt network with the
   [`opt-in-network`](https://docs.symbiotic.fi/guides/cli/#opt-in-network)
   command;
3. opt-in to any vault using the
   [`opt-in-vault`](https://docs.symbiotic.fi/guides/cli/#opt-in-vault) command;
4. deposit collateral into the vault using the
   [`deposit`](https://docs.symbiotic.fi/guides/cli/#deposit) command. For this deployment,
   you have to deposit `1 ether` of the collateral token.

**Internal Steps**

After having deposited collateral into a vault you need to register into
Bolt as a Symbiotic operator. We've provided a script to facilitate the
procedure. If you want to use it, please follow these steps:

1. set the operator private key to the `OPERATOR_SK` environment variable;
2. set the operator RPC URL which supports the Commitments API to the
   `OPERATOR_RPC` environment variable;
3. run the following Forge script from the `bolt-contracts` directory:

   ```bash
   forge script script/holesky/operators/RegisterSymbioticOperator.s.sol \
     --sig "S01_registerIntoBolt" \
     --rpc-url $HOLESKY_RPC \
     -vvvv \
     --broadcast
   ```

To check if your operator is correctly registered, set the operator address
in the `OPERATOR_ADDRESS` environment variable and run the following script:

```bash
forge script script/holesky/operators/RegisterSymbioticOperator.s.sol \
  --sig "S02_checkOperatorRegistration" \
  --rpc-url $HOLESKY_RPC \
  -vvvv
```

### EigenLayer Registration Steps

**External Steps**

> [!NOTE]
> The supported strategies can be found in
> [`deployments.json`](../../bolt-contracts/config/holesky/deployments.json).

If you're not registered as an operator in EigenLayer yet, you need to do so by
following [the official
guide](https://docs.eigenlayer.xyz/eigenlayer/operator-guides/operator-introduction).
This requires installing the EigenLayer CLI and opt into the protocol by
registering via the
[`DelegationManager.registerAsOperator`](https://docs.eigenlayer.xyz/eigenlayer/operator-guides/operator-installation)
function.

After that you need to deposit into a supported EigenLayer
strategy using
[`StrategyManager.depositIntoStrategy`](https://github.com/Layr-Labs/eigenlayer-contracts/blob/testnet-holesky/src/contracts/core/StrategyManager.sol#L303-L322).
This will add the deposit into the collateral of the operator so that Bolt can
read it. Note that you need to deposit a minimum of `1 ether` of the strategies
underlying token in order to opt in.

We've provided a script to facilitate the procedure. If you want to use it,
please set the operator private key to an `OPERATOR_SK` environment variable.

First, you need to first configure the deposit details in this JSON
file:

```bash
$EDITOR ./config/holesky/operators/eigenlayer/depositIntoStrategy.json
```

Note that the amount is in ether (so for 1 ether, specify `1` instead of 1e18).

Then you can run the following Forge script:

```bash
forge script script/holesky/operators/RegisterEigenLayerOperator.s.sol \
  --sig "S01_depositIntoStrategy()" \
  --rpc-url $HOLESKY_RPC \
  -vvvv \
  --broadcast
```

**Internal Steps**

After having deposited collateral into a strategy you need to register into the
Bolt AVS. We've provided a script to facilitate the procedure. If you want to
use it, please set follow these steps:

1. configure the operator details in this JSON file

   ```bash
   $EDITOR ./config/holesky/operators/eigenlayer/registerIntoBoltAVS.json
   ```

   In there you'll need to set the the following fields:

   - `rpc` -- the RPC URL of your operator which supports the Commitments API
   - `salt` -- an unique 32 bytes value to avoid replay attacks. To generate it on
     both Linux and MacOS you can run:

     ```bash
     echo -n "0x"; head -c 32 /dev/urandom | hexdump -e '32/1 "%02x" "\n"'
     ```

   - `expiry` -- the timestamp of the signature expiry in seconds. To generate it
     on both Linux and MacOS run the following command, replacing
     `<EXPIRY_TIMESTAMP>` with the desired timestamp:

     ```bash
     echo -n "0x"; printf "%064x\n" <EXPIRY_TIMESTAMP>
     ```

2. set the operator private key to an `OPERATOR_SK` environment
   variable;
3. run the following Forge script from the `bolt-contracts`
   directory:

```bash
forge script script/holesky/operators/RegisterEigenLayerOperator.s.sol \
  --sig "S02_registerIntoBoltAVS" \
  --rpc-url $HOLESKY_RPC \
  -vvvv \
  --broadcast
```

To check if your operator is correctly registered, set the operator address
in the `OPERATOR_ADDRESS` environment variable and run the following script:

```bash
forge script script/holesky/operators/RegisterEigenLayerOperator.s.sol \
  --sig "S03_checkOperatorRegistration" \
  --rpc-url $HOLESKY_RPC \
  -vvvv
```

# Off-Chain Setup

After all of the steps above have been completed, we can proceed with running the off-chain infrastructure.

There are various way to run the Bolt Sidecar depending on your preferences and your preferred signing methods:

- Docker mode (recommended)
- [Commit-Boost](https://commit-boost.github.io/commit-boost-client) mode
  (requires Docker)
- Native mode (advanced, requires building everything from source)

Running the Bolt sidecar as a standalone binary requires building it from
source. Both the standalone binary and the Docker container requires reading
signing keys from [ERC-2335](https://eips.ethereum.org/EIPS/eip-2335) keystores,
while the Commit-Boost module relies on an internal signer and a custom PBS
module instead of regular [MEV-Boost](https://boost.flashbots.net/).

In this section we're going to explore each of these options and its
requirements.

## Docker Mode (recommended)

First, make sure to have [Docker](https://docs.docker.com/engine/install/),
[Docker Compose](https://docs.docker.com/compose/install/) and
[git](https://git-scm.com/downloads) installed in your machine.

Then clone the Bolt repository by running:

```bash
git clone --branch v0.3.0-alpha htts://github.com/chainbound/bolt.git
cd bolt/testnets/holesky
```

The Docker Compose setup will spin up the Bolt sidecar along with the Bolt
MEV-Boost fork which includes supports the [Constraints API][constraints-api].

Before starting the services, you'll need to provide configuration files
containing the necessary environment variables:

1. **Bolt Sidecar Configuration:**

   Change directory to the `testnets/holesky` folder and create a
   `bolt-sidecar.env` file starting from the reference template:

   ```bash
   cd testnets/holesky
   cp bolt-sidecar.env.example bolt-sidecar.env
   ```

   Next up, fill out the values that are left blank. Please also review the
   default values and see that they work for your setup. For proper
   configuration of the signing options, please refer to the [Delegations and
   Signing](#delegations-and-signing-options-for-native-and-docker-compose-mode)
   section of this guide.

2. **MEV-Boost Configuration:**

   Change directory to the `testnets/holesky` folder if you haven't already and
   copy over the example configuration file:

   ```bash
   cp ./mev-boost.env.example ./mev-boost.env
   ```

Then configure it accordingly and review the default values chosen.

If you prefer not to restart your beacon node, follow the instructions in the
[Avoid Restarting the Beacon Node](#avoid-restarting-the-beacon-node) section.

Once the configuration files are in place, make sure you are in the
`testnets/holesky` directory and then run:

```bash
docker compose --env-file bolt-sidecar.env up -d
```

The docker compose setup comes with various observability tools, such as
Prometheus and Grafana. It also comes with some pre-built dashboards which you
can find at `http://localhost:28017`.

## Commit-Boost Mode

> [!IMPORTANT]
> Running with commit-boost is still in the process of being tested. Keep track of the issue
> here: https://github.com/chainbound/bolt/issues/328.

First download the `commit-boost-cli` binary from the Commit-Boost [official
releases page](https://github.com/Commit-Boost/commit-boost-client/releases)

A commit-boost configuration file with Bolt support is provided at
[`cb-bolt-config.toml`](./commit-boost/cb-bolt-config.toml). This file has support for the
custom PBS module ([bolt-boost](../../bolt-boost)) that implements the
[Constraints API][constraints-api], as well
as the [bolt-sidecar](../../bolt-sidecar) module. This file can be used as a
template for your own configuration.

The important fields to configure are under the `[modules.env]` section of the
`BOLT` module, which contain the environment variables to configure the bolt
sidecar:

```toml
[modules.env]
BOLT_SIDECAR_CHAIN = "holesky"

BOLT_SIDECAR_CONSTRAINTS_API = "http://cb_pbs:18550"     # The address of the PBS module (static)
BOLT_SIDECAR_BEACON_API = ""
BOLT_SIDECAR_EXECUTION_API = ""
BOLT_SIDECAR_ENGINE_API = ""                             # The execution layer engine API endpoint
BOLT_SIDECAR_JWT_HEX = ""                                # The engine JWT used to authenticate with the engine API
BOLT_SIDECAR_BUILDER_PROXY_PORT = "18551"                # The port on which the sidecar builder-API will listen on. This is what your beacon node should connect to.
BOLT_SIDECAR_FEE_RECIPIENT = ""                          # The fee recipient
```

To initialize commit-boost, run the following command:

```bash
commit-boost init --config cb-bolt-config.toml
```

This will create three files:

- `cb.docker-compose.yml`: which contains the full setup of the Commit-Boost services
- `.cb.env`: with local env variables, including JWTs for modules
- `target.json`: which enables dynamic discovery of services for metrics scraping via Prometheus

**Running**

The final step is to run the Commit-Boost services. This can be done with the following command:

```bash
commit-boost start --docker cb.docker-compose.yml --env .cb.env
```

This will run all modules in Docker containers.

> [!IMPORTANT]
> The `bolt-sidecar` service will be exposed at 18551 by default, set
> with `BOLT_SIDECAR_BUILDER_PROXY_PORT`, and your beacon node MUST be
> configured to point the `builder-api` to this port for Bolt to work.

**Observability**

Commit-Boost comes with various observability tools, such as Prometheus,
cadvisor, and Grafana. It also comes with some pre-built dashboards, which can
be found in the `commit-boost/grafana` directory.

To update these dashboards, run the following command from the `commit-boost`
directory:

`bash ./update-grafana.sh `
In this directory, you can also find a Bolt dashboard, which will be launched
alongside the other dashboards.

## Native Mode (advanced)

For running the Bolt Sidecar as a standalone binary you need to have the
following dependencies installed:

- [git](https://git-scm.com/downloads);
- [Rust](https://www.rust-lang.org/tools/install).
- [Golang](https://golang.org/doc/install).

Depending on your platform you may need to install additional dependencies.

<details>
<summary><b>Linux</b></summary>

Debian-based distributions:

```bash
sudo apt update && sudo apt install -y git build-essential libssl-dev build-essential ca-certificates
```

Fedora/Red Hat/CentOS distributions:

```bash
sudo dnf groupinstall "Development Tools" && sudo dnf install -y git openssl-devel ca-certificates pkgconfig
```

Arch/Manjaro-based distributions:

```bash
sudo pacman -Syu --needed base-devel git openssl ca-certificates pkgconf
```

Alpine Linux

```bash
sudo apk add git build-base openssl-dev ca-certificates pkgconf
```

</details>

<br>

<details>
  <summary><b>MacOS</b></summary>

On MacOS after installing XCode Command Line tools (equivalent to `build-essential` on Linux) you can install the other dependencies with [Homebew](https://brew.sh/):

```zsh
xcode-select --install
brew install pkg-config openssl
```

</details>

---

After having installed the dependencies you can clone the Bolt repository by
running:

```bash
git clone --branch v0.3.0 https://github.com/chainbound/bolt.git && cd bolt
```

### Building and running the MEV-Boost fork binary

The Bolt protocol relies on a modified version of
[MEV-Boost](https://boost.flashbots.net/) that supports the [Constraints
API][constraints-api]. This modified version is
available in the `mev-boost` directory of the project and can be built by
running

```bash
make build
```

in the `mev-boost` directory. The output of the command is a `mev-boost` binary.
To run the `mev-boost` binary please read the official [documentation](https://boost.flashbots.net/).

If you're already running MEV-Boost along with your beacon client it is
recommended to choose another port this service in order to [avoid restarting
your beacon client](#avoid-restarting-the-beacon-node). Check out the linked
section for more details.

### Building and running the Bolt sidecar binary

Then you can build the Bolt sidecar by running:

```bash
cargo build --release && mv target/release/bolt-sidecar .
```

In order to run correctly the sidecar you need to provide either a list command
line options or a configuration file (recommended). All the options available
can be found by running `./bolt-sidecar --help`, or you can find them in the
[reference](#command-line-options) section of this guide.

#### Configuration file

You can use a `.env` file to configure the sidecar, for which you can
find a template in the `.env.example` file.

Please read the section on [Delegations and Signing](#delegations-and-signing-options-for-native-and-docker-compose-mode)
to configure such sidecar options properly.

After you've set up the configuration file you can run the Bolt sidecar with

```bash
./bolt-sidecar
```

### Observability

The bolt sidecar comes with various observability tools, such as Prometheus
and Grafana. It also comes with some pre-built dashboards, which can
be found in the `grafana` directory.

To run these dashboards change directory to the `bolt-sidecar/infra` folder and
run:

```bash
docker compose -f telemetry.compose.yml up -d
```

To stop the services run:

```bash
docker compose -f telemetry.compose.yml down
```

# Reference

## Command-line options

For completeness, here are all the command-line options available for the Bolt
sidecar. You can see them in your terminal by running the Bolt sidecar binary
with the `--help` flag:

<details>
<summary>CLI help Reference</summary>

```text

Command-line options for the Bolt sidecar

Usage: bolt-sidecar [OPTIONS] --engine-jwt-hex <ENGINE_JWT_HEX> --fee-recipient <FEE_RECIPIENT> --builder-private-key <BUILDER_PRIVATE_KEY> --commitment-private-key <COMMITMENT_PRIVATE_KEY> <--constraint-private-key <CONSTRAINT_PRIVATE_KEY>|--commit-boost-signer-url <COMMIT_BOOST_SIGNER_URL>|--keystore-password <KEYSTORE_PASSWORD>|--keystore-secrets-path <KEYSTORE_SECRETS_PATH>>

Options:
      --port <PORT>
      Port to listen on for incoming JSON-RPC requests of the Commitments API. This port should be open on your firewall in order to receive external requests!

          [env: BOLT_SIDECAR_PORT=]
          [default: 8017]

      --execution-api-url <EXECUTION_API_URL>
          Execution client API URL

          [env: BOLT_SIDECAR_EXECUTION_API_URL=]
          [default: http://localhost:8545]

      --beacon-api-url <BEACON_API_URL>
          URL for the beacon client

          [env: BOLT_SIDECAR_BEACON_API_URL=]
          [default: http://localhost:5052]

      --engine-api-url <ENGINE_API_URL>
          Execution client Engine API URL. This is needed for fallback block building and must be a synced Geth node

          [env: BOLT_SIDECAR_ENGINE_API_URL=]
          [default: http://localhost:8551]

      --constraints-api-url <CONSTRAINTS_API_URL>
          URL to forward the constraints produced by the Bolt sidecar to a server supporting the Constraints API, such as an MEV-Boost fork

          [env: BOLT_SIDECAR_CONSTRAINTS_API_URL=]
          [default: http://localhost:18551]

      --constraints-proxy-port <CONSTRAINTS_PROXY_PORT>
          The port from which the Bolt sidecar will receive Builder-API requests from the Beacon client

          [env: BOLT_SIDECAR_CONSTRAINTS_PROXY_PORT=]
          [default: 18550]

      --engine-jwt-hex <ENGINE_JWT_HEX>
          The JWT secret token to authenticate calls to the engine API.

          It can either be a hex-encoded string or a file path to a file containing the hex-encoded secret.

          [env: BOLT_SIDECAR_ENGINE_JWT_HEX=]

      --fee-recipient <FEE_RECIPIENT>
          The fee recipient address for fallback blocks

          [env: BOLT_SIDECAR_FEE_RECIPIENT=]

      --builder-private-key <BUILDER_PRIVATE_KEY>
          Secret BLS key to sign fallback payloads with

          [env: BOLT_SIDECAR_BUILDER_PRIVATE_KEY=]

      --commitment-private-key <COMMITMENT_PRIVATE_KEY>
          Secret ECDSA key to sign commitment messages with. The public key associated to it must be then used when registering the operator in the `BoltManager` contract

          [env: BOLT_SIDECAR_COMMITMENT_PRIVATE_KEY=]

      --max-commitments-per-slot <MAX_COMMITMENTS_PER_SLOT>
          Max number of commitments to accept per block

          [env: BOLT_SIDECAR_MAX_COMMITMENTS=]
          [default: 128]

      --max-committed-gas-per-slot <MAX_COMMITTED_GAS_PER_SLOT>
          Max committed gas per slot

          [env: BOLT_SIDECAR_MAX_COMMITTED_GAS=]
          [default: 10000000]

      --min-priority-fee <MIN_PRIORITY_FEE>
          Min priority fee to accept for a commitment

          [env: BOLT_SIDECAR_MIN_PRIORITY_FEE=]
          [default: 1000000000]

      --chain <CHAIN>
          Chain on which the sidecar is running

          [env: BOLT_SIDECAR_CHAIN=]
          [default: mainnet]
          [possible values: mainnet, holesky, helder, kurtosis]

      --commitment-deadline <COMMITMENT_DEADLINE>
          The deadline in the slot at which the sidecar will stop accepting new commitments for the next block (parsed as milliseconds)

          [env: BOLT_SIDECAR_COMMITMENT_DEADLINE=]
          [default: 8000]

      --slot-time <SLOT_TIME>
          The slot time duration in seconds. If provided, it overrides the default for the selected [Chain]

          [env: BOLT_SIDECAR_SLOT_TIME=]
          [default: 12]

      --constraint-private-key <CONSTRAINT_PRIVATE_KEY>
          Private key to use for signing constraint messages

          [env: BOLT_SIDECAR_CONSTRAINT_PRIVATE_KEY=]

      --commit-boost-signer-url <COMMIT_BOOST_SIGNER_URL>
          URL for the commit-boost sidecar

          [env: BOLT_SIDECAR_CB_SIGNER_URL=]

      --commit-boost-jwt-hex <COMMIT_BOOST_JWT_HEX>
          JWT in hexadecimal format for authenticating with the commit-boost service

          [env: BOLT_SIDECAR_CB_JWT_HEX=]

      --keystore-password <KEYSTORE_PASSWORD>
          The password for the ERC-2335 keystore. Reference: https://eips.ethereum.org/EIPS/eip-2335

          [env: BOLT_SIDECAR_KEYSTORE_PASSWORD=]

      --keystore-secrets-path <KEYSTORE_SECRETS_PATH>
          The path to the ERC-2335 keystore secret passwords Reference: https://eips.ethereum.org/EIPS/eip-2335

          [env: BOLT_SIDECAR_KEYSTORE_SECRETS_PATH=]

      --keystore-path <KEYSTORE_PATH>
          Path to the keystores folder. If not provided, the default path is used

          [env: BOLT_SIDECAR_KEYSTORE_PATH=]

      --delegations-path <DELEGATIONS_PATH>
          Path to the delegations file. If not provided, the default path is used

          [env: BOLT_SIDECAR_DELEGATIONS_PATH=]

      --metrics-port <METRICS_PORT>
          The port on which to expose Prometheus metrics

          [env: BOLT_SIDECAR_METRICS_PORT=]
          [default: 3300]

      --disable-metrics
          [env: BOLT_SIDECAR_DISABLE_METRICS=]

  -h, --help
          Print help (see a summary with '-h')

```

</details>

## Delegations and signing options for Native and Docker Compose Mode

As mentioned in the [prerequisites](#prerequisites) section, the Bolt sidecar
can sign commitments with a delegated set of private keys on behalf of active
Ethereum validators.

> [!IMPORTANT]
> This is the recommended way to run the Bolt sidecar as it
> doesn't expose the active validator signing keys to any additional risk.

In order to create these delegation you can use the `bolt` CLI binary.
If you don't want to use it you can skip the following section.

### `bolt` CLI

`bolt` CLI is an offline tool for safely generating delegation and revocation messages
signed with a BLS12-381 key for the [Constraints API][constraints-api]
in [Bolt][bolt].

The tool supports three key sources:

- **Secret Keys**: A list of BLS private keys provided directly as hex-strings.
- **Local Keystore**: A EIP-2335 keystore that contains an encrypted BLS private keys.
- **Dirk**: A remote Dirk server that provides the BLS signatures for the delegation messages.

and outputs a JSON file with the delegation/revocation messages to the provided
`<DELEGATEE_PUBKEY>` for the given chain.

#### Installation and usage

Prerequisites:

- [Rust toolchain](https://www.rust-lang.org/tools/install)
- [Protoc](https://grpc.io/docs/protoc-installation/)

Once you have the necessary prerequisites, you can build the binary
in the following way:

```shell
# clone the Bolt repository if you haven't already
git clone git@github.com:chainbound/bolt.git

# navigate to the Bolt CLI package directory
cd bolt-cli

# build and install the binary on your machine
cargo install --path . --force

# test the installation
bolt --version
```

The binary can be used with the following command:

```shell
bolt delegate --delegatee-pubkey <DELEGATEE_PUBKEY>
              --out <OUTPUT_FILE>
              --chain <CHAIN>
              <KEY_SOURCE>
              <KEY_SOURCE_OPTIONS>
```

where:

- `<DELEGATEE_PUBKEY>` is the public key of the delegatee.
- `<OUTPUT_FILE>` is the path to the file where the delegation JSON messages will be written.
- `<CHAIN>` is the chain for which the delegations are being generated (e.g. Holesky).
- `<KEY_SOURCE>` is the key source to use for generating the delegations. It can be one of:
  - `secret-keys`: A list of BLS private keys provided directly as hex-strings.
  - `local-keystore`: A EIP-2335 keystore that contains an encrypted BLS private keys.
  - `dirk`: A remote Dirk server that provides the BLS signatures for the delegation messages.

You can also find more information about the available key source
options by running `bolt delegate <KEY_SOURCE> --help`.

Here you can see usage examples for each key source:

<details>
<summary>Usage</summary>

```text
❯ bolt-cli delegate --help
Generate BLS delegation or revocation messages
Usage: bolt-cli delegate [OPTIONS] --delegatee-pubkey <DELEGATEE_PUBKEY> <COMMAND>
Commands:
secret-keys     Use local secret keys to generate the signed messages
local-keystore  Use an EIP-2335 filesystem keystore directory to generate the signed messages
dirk            Use a remote DIRK keystore to generate the signed messages
help            Print this message or the help of the given subcommand(s)
Options:
    --delegatee-pubkey <DELEGATEE_PUBKEY>
        The BLS public key to which the delegation message should be signed
        [env: DELEGATEE_PUBKEY=]
    --out <OUT>
        The output file for the delegations
        [env: OUTPUT_FILE_PATH=]
        [default: delegations.json]
    --chain <CHAIN>
        The chain for which the delegation message is intended
        [env: CHAIN=]
        [default: mainnet]
        [possible values: mainnet, holesky, helder, kurtosis]
    --action <ACTION>
        The action to perform. The tool can be used to generate delegation or revocation messages (default: delegate)
        [env: ACTION=]
        [default: delegate]
        Possible values:
        - delegate: Create a delegation message
        - revoke:   Create a revocation message
-h, --help
        Print help (see a summary with '-h')
```

</details>

<details>
<summary>Examples</summary>

1. Generating a delegation using a local BLS secret key

```text
bolt-cli delegate \
  --delegatee-pubkey 0x8d0edf4fe9c80cd640220ca7a68a48efcbc56a13536d6b274bf3719befaffa13688ebee9f37414b3dddc8c7e77233ce8 \
  --chain holesky \
  secret-keys --secret-keys 642e0d33fde8968a48b5f560c1b20143eb82036c1aa6c7f4adc4beed919a22e3
```

2. Generating a delegation using an ERC-2335 keystore directory

```text
bolt-cli delegate \
 --delegatee-pubkey 0x8d0edf4fe9c80cd640220ca7a68a48efcbc56a13536d6b274bf3719befaffa13688ebee9f37414b3dddc8c7e77233ce8 \
 --chain holesky \
 local-keystore --path test_data/lighthouse/validators --password-path test_data/lighthouse/secrets
```

3. Generating a delegation using a remote DIRK keystore

```text
bolt-cli delegate \
  --delegatee-pubkey 0x83eeddfac5e60f8fe607ee8713efb8877c295ad9f8ca075f4d8f6f2ae241a30dd57f78f6f3863a9fe0d5b5db9d550b93 \
  dirk --url https://localhost:9091 \
  --client-cert-path ./test_data/dirk/client1.crt \
  --client-key-path ./test_data/dirk/client1.key \
  --ca-cert-path ./test_data/dirk/security/ca.crt \
  --wallet-path wallet1 --passphrases secret
```

</details>

<details>
<summary>Keystore-specific instructions</summary>

When using the `keystore` key source, the `--path` flag should point to the
directory containing the encrypted keypair directories.

The keystore folder must adhere to the following structure:

```text
${KEYSTORE_PATH}
|-- 0x81b676591b823270a3284ace7d81cbce2d6cdce55bb0e053874d7e3a08f729453009d3e662ec3130379f43c0f3210b6d
|   `-- voting-keystore.json
|-- 0x81ea9f74ef7d935b807474e38954ae3934856219a23e074954b2e860c5a3c400f9aedb42cd27cb4ceb697ca36d1e58cb
|   `-- voting-keystore.json
|-- ...
    `-- ...
```

where the folder names are the public keys and inside every
folder there is a single JSON file containing the keystore file.

In case of validator-specific passwords (e.g. Lighthouse format) the
`--password-path` flag must be used instead of `--password`, pointing to the
directory containing the password files.

The passwords folder must adhere to a certain structure as well, as shown below.

```
${KEYSTORE_PATH}
|-- 0x81b676591b823270a3284ace7d81cbce2d6cdce55bb0e053874d7e3a08f729453009d3e662ec3130379f43c0f3210b6d
|-- 0x81ea9f74ef7d935b807474e38954ae3934856219a23e074954b2e860c5a3c400f9aedb42cd27cb4ceb697ca36d1e58cb
|-- ...
    `-- ...
```

That is, the password files should be named after the public key and each file
should just contain one line with the password in plain text. The files
themselves don't need a particular file extension.

</details>

---

Now that you have generated the delegation messages you can provide them to the
sidecar using the `--delegations-path` flag (or `BOLT_SIDECAR_DELEGATIONS_PATH`
env). When doing so the sidecar will check if they're indeed valid messages and
will keep in memory the association between the delegator and the delegatee.

However in order to sign the commitments you still need to provide the signing
key of the delegatee. There are two ways to do so, as explored in the sections
below.

### Using a private key directly

As you can see in the [command line options](#command-line-options) section you
can pass directly the private key as a hex-encoded string to the Bolt sidecar
using the `--constraint-private-key` flag (or
`BOLT_SIDECAR_CONSTRAINT_PRIVATE_KEY` env).

This is the simplest setup and can be used in case if all the delegations messages
point to the same delegatee or if you're running the sidecar with a single active
validator.

### Using a ERC-2335 Keystore

The Bolt sidecar supports [ERC-2335](https://eips.ethereum.org/EIPS/eip-2335)
keystores for loading signing keypairs. In order to use them you need to provide
the `--keystore-path` (`BOLT_SIDECAR_KEYSTORE_PATH` env) pointing to the folder
containing the keystore files and the `--keystore-password` or
`keystore-secrets-path` flag (`BOLT_SIDECAR_KEYSTORE_PASSWORD` or
`BOLT_SIDECAR_SECRETS_PATH` env respectively) pointing to the folder containing
the password file.

Both the `keys` and `passwords` folders must adhere to the structure outlined
in the [Installation and Usage](#installation-and-usage) section.

## Avoid restarting the beacon node

As mentioned in the [prerequisites](#prerequisites) section, in order to run the
sidecar correctly it might be necessary to restart your beacon client. That is
because you need to configure the `--builder` flag (or equivalent) to point to
the Bolt sidecar endpoint.

However if you're already running a PBS sidecar like
[MEV-Boost](https://boost.flashbots.net/) on the same machine then you can avoid
the restart by following this steps when starting the Bolt sidecar:

1. Set the `--constraints-proxy-port` flag (or
   `BOLT_SIDECAR_CONSTRAINTS_PROXY_PORT` env) to the port previously occupied by
   MEV-Boost.
2. Build the Bolt MEV-Boost fork binary or pull the Docker image and start it
   using another port
3. Set the `--constraints-api-url` flag (or `BOLT_SIDECAR_CONSTRAINTS_API_URL`
   env) to point to the Bolt MEV-Boost instance.