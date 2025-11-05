MEMORY
{
  /* NOTE 1 K = 1 KiBi = 1024 bytes */
  /* STM32C092RC: 256KB Flash (2KB page size), 30KB RAM */
  FLASH                             : ORIGIN = 0x08000000, LENGTH = 24K
  BOOTLOADER_STATE                  : ORIGIN = 0x08006000, LENGTH = 4K
  ACTIVE                            : ORIGIN = 0x08007000, LENGTH = 100K
  DFU                               : ORIGIN = 0x08020000, LENGTH = 102K
  RAM                         (rwx) : ORIGIN = 0x20000000, LENGTH = 30K
}

__bootloader_state_start = ORIGIN(BOOTLOADER_STATE) - ORIGIN(FLASH);
__bootloader_state_end = ORIGIN(BOOTLOADER_STATE) + LENGTH(BOOTLOADER_STATE) - ORIGIN(FLASH);

__bootloader_active_start = ORIGIN(ACTIVE) - ORIGIN(FLASH);
__bootloader_active_end = ORIGIN(ACTIVE) + LENGTH(ACTIVE) - ORIGIN(FLASH);

__bootloader_dfu_start = ORIGIN(DFU) - ORIGIN(FLASH);
__bootloader_dfu_end = ORIGIN(DFU) + LENGTH(DFU) - ORIGIN(FLASH);
