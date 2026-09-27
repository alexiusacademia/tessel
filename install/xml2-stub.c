/* Stand-ins for the libxml2 functions LLVM's Windows manifest merger uses.
   LLVM's official Windows build lists libxml2 (xml2s.lib) as a dependency
   but doesn't ship it. Tessel never merges manifests, so these just fail. */
#include <stdlib.h>

void (*xmlFree)(void *) = free;

void *xmlAddChild(void *parent, void *child) { return 0; }
void *xmlCopyNamespace(void *ns) { return 0; }
void xmlDocDumpFormatMemoryEnc(void *doc, void **mem, int *size, const char *enc, int format) {
    if (mem) *mem = 0;
    if (size) *size = 0;
}
void *xmlDocGetRootElement(void *doc) { return 0; }
void *xmlDocSetRootElement(void *doc, void *root) { return 0; }
void xmlFreeDoc(void *doc) {}
void xmlFreeNode(void *node) {}
void xmlFreeNs(void *ns) {}
void *xmlNewDoc(const char *version) { return 0; }
void *xmlNewNs(void *node, const char *href, const char *prefix) { return 0; }
void *xmlNewProp(void *node, const char *name, const char *value) { return 0; }
void *xmlReadMemory(const char *buffer, int size, const char *url, const char *enc, int options) { return 0; }
void xmlSetGenericErrorFunc(void *ctx, void *handler) {}
char *xmlStrdup(const char *s) { return 0; }
void xmlUnlinkNode(void *node) {}
