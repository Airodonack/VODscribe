# The network's tests

- `resume.rs`: the specs of `download_resumable`: a download carries on from the bytes it had (one that ends early too), a damaged one or a wrong part starts over, reads stop at the size, a link is never written through, and nothing reaches its file unchecked.
