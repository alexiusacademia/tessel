// A C entry point to LLVM's linker, lld, which is built into `tessel` in
// release builds (the `bundled-linker` feature), so building apps needs no
// system linker. See link_with_lld in main.rs.

#include "lld/Common/Driver.h"
#include "llvm/Support/raw_ostream.h"

#include <cstdlib>
#include <cstring>
#include <string>

#if defined(TESSEL_LLD_MACHO)
LLD_HAS_DRIVER(macho)
#endif
#if defined(TESSEL_LLD_COFF)
LLD_HAS_DRIVER(coff)
LLD_HAS_DRIVER(mingw)
#endif

// Links with `args` (args[0] names the flavor, e.g. "ld64.lld"). Returns the
// exit code, and the linker's messages in `*output` (free with tessel_lld_free).
extern "C" int tessel_lld_link(const char *const *args, int count, char **output) {
    std::string messages;
    llvm::raw_string_ostream out(messages);
    const lld::DriverDef drivers[] = {
#if defined(TESSEL_LLD_MACHO)
        {lld::Darwin, &lld::macho::link},
#endif
#if defined(TESSEL_LLD_COFF)
        {lld::WinLink, &lld::coff::link},
        {lld::MinGW, &lld::mingw::link},
#endif
    };
    lld::Result result = lld::lldMain(llvm::ArrayRef<const char *>(args, count), out, out, drivers);
    out.flush();
    *output = static_cast<char *>(std::malloc(messages.size() + 1));
    std::memcpy(*output, messages.c_str(), messages.size() + 1);
    return result.retCode;
}

extern "C" void tessel_lld_free(char *output) { std::free(output); }
