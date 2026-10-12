# CLang demo
(var.store(operation=add, True: dnc(var.all() as object))) => response = string("World");
global(var.get(object){response});
(fn) => allerg{
    # CLang does not have it's own captilization features so we need to make our own
    for first(letter) in arg(response){
        (var.get(object) => response: r);
        global(r){
            condition = {
                only.for(..) then {
                    remove(property(global) from r);
                };
            };
        };
        # CLang has ID's for each letter so we auto sort out he first letter from the array that we scan through and find the letter with the same ID which is our capital because they're the same ID but the capital has a .cap at the end of it's ID
        (var.store(operation=add, True: dnc(var.all() as object))) => find = array[
            (fn.alphabet())
        ];
        (var.transform(operation=transform, True: dnc(var.all() as object))) => find => remove(r.first(letter));
        (var.transform(operation=transform, True: dnc(var.all() as object))) => find => change(all(letter) => {that(letter.fn(stringID))});
        (var.store(operation=add, True: dnc(var.all() as object))) => rfirstletterid => change(r.first(letter) => {that(letter.fn(stringID))});
        if (var.get(object){rfirstletterid}) in find{
            condition = {
                without.string('.cap') in rfirstletterid;
            };
            # Bring ID's back to original letter
            for all(letter) in r{
                # stitch them together
                len(letts)
            };
        };
        (fn.end(current));
    };
    (fn.end(&return r);
};

# boot function
# CLang prints variable response after giving it to allerg field
(fn) => boot{
    (var.get(object) => response: r);
    fn(allerg(r)){all => (fn.permissions(for function) => object)};
    (fn.run()) => show(allerg(r));
};

fn(boot){all => (fn.permissions(for function) => object)};
