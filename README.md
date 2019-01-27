# sockfw 

A simple forwarder for SSL, TCP and UNIX sockets. Primarily used to forward Docker control socket to the outer world.

Make it easier to start a new docker container exposing the docker-engine interface than configuring
the docker-engine to use your SSL certificate.

This module does not use dyn or any indirection for it's plugin interfaces, thus
the output code size will suffer but the performance is increased instead. Increased
code size must not affect the performance due to cache thrashing as the actual paths
use are going to be the ones for activated plugins.

Practically, although `sockfw` uses dynamic memory allocation - it should not be too 
hard to convert it to purely static memory allocation.

## How to use

Use cargo to build the package and then install it

### Help info

```shell
>> sfw --help
universal forwarder 0.1
Andrey Cizov <acizov@gmail.com>

USAGE:
    sfw [OPTIONS] [SUBCOMMAND]

FLAGS:
    -h, --help       Prints help information
    -V, --version    Prints version information

OPTIONS:
        --capacity <capacity>                    [default: 2048]
        --client-buffer <client_buffer_size>     [default: 8192]
        --event-buffer <event_buffer_size>       [default: 2048]

SUBCOMMANDS:
    help    Prints this message or the help of the given subcommand(s)
    tcp     
    tls 
```

### Example output config

```shell
>> sfw tls --help
sfw-tls 

USAGE:
   sfw tls <addr> <ca> <cert> <privkey> [SUBCOMMAND]

FLAGS:
   -h, --help       Prints help information
   -V, --version    Prints version information

ARGS:
   <addr>       
   <ca>         
   <cert>       
   <privkey>    

SUBCOMMANDS:
   help    Prints this message or the help of the given subcommand(s)
   unix
```

## License

`sockfw` is licensed under either of

 * Apache License, Version 2.0, ([LICENSE-APACHE](LICENSE-APACHE) or
   http://www.apache.org/licenses/LICENSE-2.0)
 * MIT license ([LICENSE-MIT](LICENSE-MIT) or
   http://opensource.org/licenses/MIT)

at your option.

### Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in Serde by you, as defined in the Apache-2.0 license, shall be
dual licensed as above, without any additional terms or conditions.
