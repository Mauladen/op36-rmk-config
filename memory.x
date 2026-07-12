MEMORY
{
  /* NOTE 1 K = 1 KiB = 1024 bytes */
  /* RP2040 (3W6HS left half): 2M flash with the 256-byte stage-2 bootloader,
     256K SRAM. The boot2 blob is provided by embassy-rp (.boot2 section). */
  BOOT2 : ORIGIN = 0x10000000, LENGTH = 0x100
  FLASH : ORIGIN = 0x10000100, LENGTH = 2048K - 0x100
  RAM : ORIGIN = 0x20000000, LENGTH = 256K
}

EXTERN(BOOT2_FIRMWARE)

SECTIONS {
  /* ### Boot loader */
  .boot2 ORIGIN(BOOT2) :
  {
    KEEP(*(.boot2));
  } > BOOT2
} INSERT BEFORE .text;
