## 1. Planning from headers (`summrise-dicom`)

- [ ] 1.1 `plan_series`: display order and volume-geometry verdict from indexed frame planes and instance numbers; unit tests give today's order for the synthetic CT, MR, localizer and colour series
- [ ] 1.2 Per-file decode function (all frames of one file) with the existing panic isolation
- [ ] 1.3 Value-range estimate from Bits Stored, Pixel Representation and rescale values

## 2. Incremental store and loader (`summrise-core`)

- [ ] 2.1 Write-once slot store shared by loader and UI; failed images recorded with their reason
- [ ] 2.2 Coordinator with the displayed index, runs of consecutive images, a single file reader and parallel decoding
- [ ] 2.3 Pause on series switch and resume on return; whole-series eviction under the memory budget
- [ ] 2.4 Volume assembled when the last slot fills; initial window fixed from the first image

## 3. UI

- [ ] 3.1 Placeholders for unloaded and failed images; progress in the viewport and the series browser
- [ ] 3.2 Reformat controls disabled with "Loading n / N" until complete and enabled automatically afterwards
- [ ] 3.3 Inspector, overlays and status-bar read-out work with a partly loaded series

## 4. Verification

- [ ] 4.1 Test with an artificially slow image source: first image visible after one decode; counter and positions stable
- [ ] 4.2 Test re-prioritisation: after a jump, the target image is decoded before the gap
- [ ] 4.3 Test pause and resume, and that W 1500 / L −600 set during loading is unchanged at completion
- [ ] 4.4 Final state identical to non-progressive loading for every synthetic series (order, volume, value range)
- [ ] 4.5 Time to first image measured from a real optical drive (needs hardware; cannot be verified on this Mac without a drive, tracked with `field-validation`)
