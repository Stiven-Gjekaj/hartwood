/*
 * The virtual memory tests of riscv-tests include <string.h>, and a compiler
 * for bare metal with no C library has none. env/v/string.c defines each of
 * these functions, so this file only declares them.
 */
#ifndef HARTWOOD_STRING_H
#define HARTWOOD_STRING_H
#include <stddef.h>
void *memcpy(void *dest, const void *src, size_t len);
void *memset(void *dest, int byte, size_t len);
int memcmp(const void *s1, const void *s2, size_t n);
size_t strlen(const char *s);
int strcmp(const char *s1, const char *s2);
char *strcpy(char *dest, const char *src);
#endif
