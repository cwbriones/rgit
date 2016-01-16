Rgit
=====

This project is a primarily a product of the excellent article
[git clone in Haskell from the ground up](http://stefan.saasen.me/articles/git-clone-in-haskell-from-the-bottom-up/#implementing_pack_file_negotiation) and my desire for a something somewhat larger than a toy project in Rust.

Hopefully by the end of this you should be able to successfully run the following to create a valid git repo:
```bash
rgit clone git://github.com/cwbriones/rgit.git
```

## Todo
- [x] Transport Protocol and Pack Wire Protocol
  - [x] Reference Discovery (ls-remote)
  - [x] Capabilities
  - [x] Packfile Negotiation
- [x] Delta Encoding
- [x] Repo and Object Storage Format
- [x] Refs
- [ ] Working Copy and Index
- [ ] Clone Command
