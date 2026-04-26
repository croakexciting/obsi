MEMORY
{
  RAM : ORIGIN = 0x40000000, LENGTH = 128M
}

ENTRY(_start)

SECTIONS
{
  . = ORIGIN(RAM);
  PROVIDE(__executable_start = .);

  .text :
  {
    *(.text.entry)
    *(.text .text.*)
  } > RAM
  PROVIDE(__etext = .);

  .eh_frame :
  {
    . = ALIGN(8);
    PROVIDE(__eh_frame = .);
    KEEP(*(.eh_frame))
    KEEP(*(.eh_frame.*))
  } > RAM

  .eh_frame_hdr :
  {
    KEEP(*(.eh_frame_hdr))
    KEEP(*(.eh_frame_hdr.*))
  } > RAM

  .gcc_except_table :
  {
    KEEP(*(.gcc_except_table))
    KEEP(*(.gcc_except_table.*))
  } > RAM

  .rodata :
  {
    *(.rodata .rodata.*)
  } > RAM

  .data :
  {
    *(.data .data.*)
  } > RAM

  .bss :
  {
    *(.bss .bss.* COMMON)
  } > RAM

  /DISCARD/ : { *(.ARM.exidx) *(.ARM.extab) }
}
